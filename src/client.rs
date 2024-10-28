use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use reqwest::Method as ReqwestMethod;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{error, info};
use tungstenite::client::IntoClientRequest;

// we will use tungstenite for websocket client impl (same library as what axum is using)
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::context::ClientContext;
use crate::message::{ResponseLine, TunnelMessage};
use crate::message::{StatusLine, WebhookHeader};
use crate::{config::ClientConfig, token::create_auth_token, Result};

pub struct ClientTunnelReceiver {
    stream: Option<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
}

impl ClientTunnelReceiver {
    pub fn new(stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) -> Self {
        Self {
            stream: Some(stream),
        }
    }

    pub async fn read(&mut self) -> Option<tungstenite::Message> {
        if let Some(stream) = self.stream.as_mut() {
            if let Some(Ok(msg)) = stream.next().await {
                return Some(msg);
            }
        }
        None
    }
}

pub struct ClientTunnelSender {
    stream: Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>>,
}

impl ClientTunnelSender {
    pub fn new(
        stream: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    ) -> Self {
        Self {
            stream: Some(stream),
        }
    }

    pub async fn send(&mut self, msg: tungstenite::Message) -> Result<()> {
        if let Some(stream) = self.stream.as_mut() {
            if let Err(e) = stream.send(msg).await {
                let msg = format!("Error sending message: {e}");
                return Err(msg.into());
            }
        }
        Ok(())
    }
}

pub async fn start_client(ctx: Arc<ClientContext>) {
    let res = ws_main(ctx).await;
    if let Err(e) = res {
        error!("{:?}", e);
    }
}

async fn ws_main(ctx: Arc<ClientContext>) -> Result<()> {
    let token = create_auth_token(ctx.config.jwt_secret.as_str()).unwrap();
    let ws_address = ctx.config.ws_address.clone();
    let mut req = ws_address.as_str().into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", token.as_str().parse().unwrap());

    let ws_stream = match connect_async(req).await {
        Ok((stream, response)) => {
            println!("Handshake for client has been completed");
            println!("Server response was {:?}", response);
            stream
        }
        Err(e) => {
            let error = format!("Websocket handshake for client failed with {:?}", e);
            return Err(error.into());
        }
    };

    let crawler = Client::new();
    let (sender, receiver) = ws_stream.split();
    let tunnel_receiver = Arc::new(Mutex::new(ClientTunnelReceiver::new(receiver)));
    let tunnel_sender = Arc::new(Mutex::new(ClientTunnelSender::new(sender)));

    let res = tokio::try_join!(
        ping_task(tunnel_sender.clone()),
        send_task(ctx.clone(), tunnel_sender, crawler),
        recv_task(ctx.clone(), tunnel_receiver)
    );

    if let Err(e) = res {
        let msg = format!("{}", e);
        return Err(msg.into());
    }

    Err("Websocket client exited.".into())
}

async fn ping_task(sender: Arc<Mutex<ClientTunnelSender>>) -> Result<()> {
    loop {
        // Send a ping message
        let res = {
            let mut stream = sender.lock().await;
            stream.send(Message::Ping("PING".into())).await
        };
        if let Err(e) = res {
            let msg = format!("Error sending ping: {}", e);
            error!("{}", msg);
            break;
        }

        // Send another in 60 seconds
        sleep(Duration::from_secs(60)).await;
    }

    Err("Ping task exited.".into())
}

async fn send_task(
    ctx: Arc<ClientContext>,
    sender: Arc<Mutex<ClientTunnelSender>>,
    crawler: Client,
) -> Result<()> {
    loop {
        let maybe_req = ctx.get_request().await;
        if let Some(message) = maybe_req {
            let ctx_clone = ctx.clone();
            let crawler_clone = crawler.clone();
            let sender_clone = sender.clone();
            tokio::spawn(async move {
                handle_forward(ctx_clone, crawler_clone, sender_clone, message).await
            });
        }
    }

    Ok(())
}

async fn recv_task(
    ctx: Arc<ClientContext>,
    receiver: Arc<Mutex<ClientTunnelReceiver>>,
) -> Result<()> {
    loop {
        let maybe_msg = {
            let mut tunnel = receiver.lock().await;
            tunnel.read().await
        };

        if let Some(msg) = maybe_msg {
            let res = handle_ws_message(ctx.clone(), msg).await;
            if res.is_break() {
                break;
            }
        }
    }

    Err("Websocket receiver task exited.".into())
}

async fn handle_ws_message(ctx: Arc<ClientContext>, msg: Message) -> ControlFlow<()> {
    match msg {
        Message::Pong(v) => {
            // Track the last pong message timestamp
            info!("Received pong with {:?}", v);
        }
        Message::Binary(b) => match TunnelMessage::from_buffer(&b[..]) {
            Ok(message) => {
                ctx.add_request(message).await;
            }
            Err(e) => {
                error!("Failed to parse binary message: {}", e);
            }
        },
        Message::Close(_) => {
            return ControlFlow::Break(());
        }
        _ => {
            // Do nothing
        }
    }

    ControlFlow::Continue(())
}

async fn handle_forward(
    ctx: Arc<ClientContext>,
    crawler: Client,
    sender: Arc<Mutex<ClientTunnelSender>>,
    message: TunnelMessage,
) -> Result<()> {
    let res = handle_target_response(crawler, ctx.config.clone(), message).await?;
    let write_res = {
        let mut stream = sender.lock().await;
        stream.send(Message::Binary(res.into_bytes())).await
    };

    if let Err(e) = write_res {
        let msg = format!("Error sending message: {}", e);
        return Err(msg.into());
    }

    Ok(())
}

async fn handle_target_response(
    crawler: Client,
    config: Arc<ClientConfig>,
    message: TunnelMessage,
) -> Result<TunnelMessage> {
    let st_opt = match message.http_line {
        StatusLine::Request(req) => Some(req),
        _ => None,
    };
    let st = st_opt.expect("Message must be a forward request.");
    let method = st.method.as_str();
    let uri = st.path.as_str();

    let req_id = message.header.id.clone();
    info!(
        "Forwarding request: {} {} ID={}",
        method,
        uri,
        req_id.to_string()
    );

    // Figure out the method
    // We assume that the target is a localhost address
    let protocol = match config.target_secure {
        true => "https",
        false => "http",
    };
    let url = format!("{}://{}{}", protocol, &config.target_address, uri);

    let mut r = crawler.request(ReqwestMethod::from_bytes(method.as_bytes()).unwrap(), url);

    // Inject headers
    for (k, v) in message.http_headers.iter() {
        if k == "host" {
            // Rename host to the proxied target host
            r = r.header("host", &config.target_address);
        } else {
            r = r.header(k, v);
        }
    }

    if message.http_body.len() > 0 {
        r = r.body(message.http_body);
    }

    let response = r.send().await;
    match response {
        Ok(res) => {
            // Build the whole response back into a TunnelMessage
            let version = format!("{:?}", res.version());
            let status_line = StatusLine::Response(ResponseLine::new(
                version,
                res.status().as_u16(),
                Some(res.status().canonical_reason().unwrap().to_string()),
            ));

            let header = WebhookHeader::new(req_id);
            let mut tunnel_res = TunnelMessage::new(header, status_line);
            tunnel_res.http_headers.extend(
                res.headers()
                    .iter()
                    .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap().to_string())),
            );

            tunnel_res.http_body = res.bytes().await.unwrap().to_vec();

            Ok(tunnel_res)
        }
        Err(e) => {
            let msg = format!("Forward request error: {}", e);
            error!(msg);
            Err(msg.into())
        }
    }
}
