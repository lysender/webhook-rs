use futures::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use reqwest::redirect;
use reqwest::Client;
use reqwest::ClientBuilder;
use reqwest::Method as ReqwestMethod;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{error, info};
use tungstenite::client::IntoClientRequest;

// we will use tungstenite for websocket client impl (same library as what axum is using)
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::config::Config;
use crate::config::ProxyTarget;
use crate::context::Context;
use webhook::message::{ResponseLine, TunnelMessage};
use webhook::message::{StatusLine, WebhookHeader};
use webhook::token::create_auth_token;
use webhook::Result;

pub struct ClientTunnelReceiver {
    stream: Option<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
}

impl ClientTunnelReceiver {
    pub fn new(stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) -> Self {
        Self {
            stream: Some(stream),
        }
    }

    pub async fn read(&mut self) -> Option<Result<Message>> {
        if let Some(stream) = self.stream.as_mut() {
            if let Some(res) = stream.next().await {
                match res {
                    Ok(msg) => return Some(Ok(msg)),
                    Err(e) => {
                        let msg = format!("Error reading message: {}", e);
                        return Some(Err(msg.into()));
                    }
                }
            }
        }
        None
    }
}

pub struct ClientTunnelSender {
    stream: Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
}

impl ClientTunnelSender {
    pub fn new(stream: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>) -> Self {
        Self {
            stream: Some(stream),
        }
    }

    pub async fn send(&mut self, msg: Message) -> Result<()> {
        if let Some(stream) = self.stream.as_mut() {
            if let Err(e) = stream.send(msg).await {
                let msg = format!("Error sending message: {e}");
                return Err(msg.into());
            }
        }
        Ok(())
    }
}

pub async fn start_client(ctx: Arc<Context>) {
    loop {
        let ctx_clone = ctx.clone();
        let res = ws_main(ctx_clone).await;
        if let Err(e) = res {
            error!("{}", e);
        }

        info!("Reconnecting in 10 seconds...");
        sleep(Duration::from_secs(10)).await;
    }
}

async fn ws_main(ctx: Arc<Context>) -> Result<()> {
    let token = create_auth_token(ctx.config.jwt_secret.as_str()).unwrap();
    let ws_address = ctx.config.server_address.clone();
    let mut req = ws_address.as_str().into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", token.as_str().parse().unwrap());

    let ws_stream = match connect_async(req).await {
        Ok((stream, _response)) => stream,
        Err(e) => {
            let error = format!("Websocket handshake for client failed with {}", e);
            return Err(error.into());
        }
    };

    ctx.verify().await;

    info!("Connected to websocket server.");

    let Ok(crawler) = ClientBuilder::new()
        .redirect(redirect::Policy::none())
        .build()
    else {
        let msg = "Failed to create crawler client.";
        return Err(msg.into());
    };

    let (sender, receiver) = ws_stream.split();
    let tunnel_receiver = Arc::new(Mutex::new(ClientTunnelReceiver::new(receiver)));
    let tunnel_sender = Arc::new(Mutex::new(ClientTunnelSender::new(sender)));

    let res = tokio::try_join!(
        ping_task(ctx.clone(), tunnel_sender.clone()),
        send_task(ctx.clone(), tunnel_sender, crawler),
        recv_task(ctx.clone(), tunnel_receiver)
    );

    if let Err(e) = res {
        let msg = format!("{}", e);
        error!("{}", msg);
    }

    ctx.unverify().await;

    Err("Websocket client exited.".into())
}

async fn ping_task(ctx: Arc<Context>, sender: Arc<Mutex<ClientTunnelSender>>) -> Result<()> {
    loop {
        if ctx.is_stale_connection(Instant::now()).await {
            error!("Connection is stale. Terminating.");
            break;
        }

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
    ctx: Arc<Context>,
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

async fn recv_task(ctx: Arc<Context>, receiver: Arc<Mutex<ClientTunnelReceiver>>) -> Result<()> {
    loop {
        let received = {
            let mut tunnel = receiver.lock().await;
            tunnel.read().await
        };

        if let Some(received_res) = received {
            match received_res {
                Ok(msg) => {
                    let res = handle_ws_message(ctx.clone(), msg).await;
                    if res.is_break() {
                        break;
                    }
                }
                Err(e) => {
                    let msg = format!("{}", e);
                    error!("{}", msg);
                    break;
                }
            }
        }
    }

    Err("Websocket receiver task exited.".into())
}

async fn handle_ws_message(ctx: Arc<Context>, msg: Message) -> ControlFlow<()> {
    match msg {
        Message::Pong(_) => {
            ctx.update_pong(Instant::now()).await;
            ControlFlow::Continue(())
        }
        Message::Binary(b) => match TunnelMessage::from_buffer(&b[..]) {
            Ok(message) => {
                ctx.add_request(message).await;
                ControlFlow::Continue(())
            }
            Err(e) => {
                error!("Failed to parse binary message: {}", e);
                ControlFlow::Continue(())
            }
        },
        Message::Close(_) => ControlFlow::Break(()),
        _ => ControlFlow::Continue(()),
    }
}

async fn handle_forward(
    ctx: Arc<Context>,
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

fn find_target<'a>(config: &'a Config, uri: &str) -> Option<&'a ProxyTarget> {
    config
        .targets
        .iter()
        .find(|target| uri.starts_with(&target.source_path))
}

async fn handle_target_response(
    crawler: Client,
    config: Arc<Config>,
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

    // Detect from which target the request is for
    // Route request to that target
    let target = find_target(&config, uri).expect("Target is required for the route.");

    // Figure out the method
    // We assume that the target is a localhost address
    let protocol = match target.secure {
        true => "https",
        false => "http",
    };

    let target_uri = uri.replace(&target.source_path, &target.dest_path);
    let url = format!("{}://{}{}", protocol, &target.host, &target_uri);

    let mut r = crawler.request(ReqwestMethod::from_bytes(method.as_bytes()).unwrap(), url);

    // Inject headers
    for (k, v) in message.http_headers.iter() {
        if k == "host" {
            // Rename host to the proxied target host
            r = r.header("host", &target.host);
        } else {
            r = r.header(k, v);
        }
    }

    // Do not follow redirects

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
