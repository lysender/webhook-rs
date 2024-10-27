use futures_util::stream::FuturesUnordered;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use reqwest::Method as ReqwestMethod;
use std::borrow::Cow;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Instant;
use std::{thread, time::Duration};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};
use tracing::{error, info};
use tungstenite::client::IntoClientRequest;
use uuid::Uuid;

// we will use tungstenite for websocket client impl (same library as what axum is using)
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::{frame::coding::CloseCode, CloseFrame, Message},
};

use tungstenite::http::{Method, Request};

use crate::context::ClientContext;
use crate::message::ResponseLine;
use crate::message::StatusLine;
use crate::message::TunnelMessage;
use crate::message::WEBHOOK_OP;
use crate::message::WEBHOOK_OP_FORWARD_RES;
use crate::message::WEBHOOK_TOKEN;
use crate::tunnel::TunnelReader;
use crate::tunnel::TunnelWriter;
use crate::Error;
use crate::{config::ClientConfig, token::create_auth_token, Result};

pub async fn start_client(ctx: Arc<ClientContext>) {
    //loop {
    //    let con = { connect(ctx.clone()).await };
    //
    //    if let Err(e) = con {
    //        error!("Connection error: {}", e);
    //        info!("Reconnecting in 10 seconds...");
    //
    //        thread::sleep(Duration::from_secs(10));
    //    }
    //}
    let res = ws_main(ctx).await;
    if let Err(e) = res {
        error!("{:?}", e);
    }
}

async fn connect(ctx: Arc<ClientContext>) -> Result<()> {
    let context = ctx.clone();
    context.reset().await;

    let config = context.config.clone();

    let stream_res = TcpStream::connect(&config.tunnel_address).await;
    let crawler = Client::new();

    match stream_res {
        Ok(stream) => {
            info!("Connected to server...");
            handle_connection(context, crawler, stream).await
        }
        Err(e) => {
            let connect_err = format!("Error connecting to the server: {}", e);
            Err(connect_err.into())
        }
    }
}

async fn handle_connection(
    ctx: Arc<ClientContext>,
    crawler: Client,
    stream: TcpStream,
) -> Result<()> {
    info!("Authenticating to server...");

    let (reader, writer) = stream.into_split();
    let tunnel_reader = Arc::new(Mutex::new(TunnelReader::new(reader)));
    let tunnel_writer = Arc::new(Mutex::new(TunnelWriter::new(writer)));

    let config = ctx.config.clone();

    let _ = authenticate(tunnel_reader.clone(), tunnel_writer.clone(), config).await?;

    let join_res = tokio::try_join!(
        handle_requests(ctx.clone(), tunnel_reader),
        handle_forwards(ctx, tunnel_writer, crawler)
    );

    if let Err(e) = join_res {
        let msg = format!("{}", e);
        return Err(msg.into());
    }

    Err("Connection closed.".into())
}

async fn ws_main(ctx: Arc<ClientContext>) -> Result<()> {
    let start_time = Instant::now();

    // Spawn serveral clients
    let mut clients = (0..2)
        .map(|i| tokio::spawn(spawn_client(ctx.clone(), i)))
        .collect::<FuturesUnordered<_>>();

    // Wait for all clients to exit
    while clients.next().await.is_some() {}

    let end_time = Instant::now();

    println!(
        "Total time taken {:?} with {} concurrent clients, should be about 6.45 seconds",
        end_time - start_time,
        10
    );

    Ok(())
}

async fn spawn_client(ctx: Arc<ClientContext>, who: usize) {
    let token = create_auth_token(ctx.config.jwt_secret.as_str()).unwrap();
    let ws_address = ctx.config.ws_address.clone();
    let mut req = ws_address.as_str().into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", token.as_str().parse().unwrap());

    let ws_stream = match connect_async(req).await {
        Ok((stream, response)) => {
            println!("Handshake for client {:?} has been completed", who);
            println!("Server response was {:?}", response);
            stream
        }
        Err(e) => {
            println!(
                "Websocket handshake for client {:?} failed with {:?}",
                who, e
            );
            return;
        }
    };

    let (mut sender, mut receiver) = ws_stream.split();

    sender
        .send(Message::Ping("Hello server".into()))
        .await
        .expect("Cannot send");

    let mut send_task = tokio::spawn(async move {
        for i in 1..30 {
            // In any websocket error, break loop
            if sender
                .send(Message::Text(format!("Message number {}...", i)))
                .await
                .is_err()
            {
                // If send fails, there is nothing we can do but exit
                return;
            }

            sleep(Duration::from_millis(3000)).await;
        }
    });

    // receiver just prints whatever it receives
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            // Print message and break if told to do so
            if process_message(msg, who).is_break() {
                break;
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
        },
        _ = (&mut recv_task) => {
            send_task.abort();
        }
    }
}

fn process_message(msg: Message, who: usize) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t) => {
            println!(">>> {who} got str: {t:?}");
        }
        Message::Binary(d) => {
            println!(">>> {} got {} bytes: {:?}", who, d.len(), d);
        }
        Message::Close(c) => {
            if let Some(cf) = c {
                println!(
                    ">>> {} got close with code {} and reason `{}`",
                    who, cf.code, cf.reason
                );
            } else {
                println!(">>> {who} somehow got close message without CloseFrame");
            }
            return ControlFlow::Break(());
        }

        Message::Pong(v) => {
            println!(">>> {who} got pong with {v:?}");
        }
        // Just as with axum server, the underlying tungstenite websocket library
        // will handle Ping for you automagically by replying with Pong and copying the
        // v according to spec. But if you need the contents of the pings you can see them here.
        Message::Ping(v) => {
            println!(">>> {who} got ping with {v:?}");
        }

        Message::Frame(_) => {
            unreachable!("This is never supposed to happen")
        }
    }
    ControlFlow::Continue(())
}

async fn authenticate(
    tunnel_reader: Arc<Mutex<TunnelReader>>,
    tunnel_writer: Arc<Mutex<TunnelWriter>>,
    config: Arc<ClientConfig>,
) -> Result<()> {
    let id = Uuid::now_v7();
    let token = create_auth_token(&config.jwt_secret)?;
    let auth_req = TunnelMessage::with_auth_token(id, token);

    {
        let mut writer = tunnel_writer.lock().await;
        let write_res = writer.write(&auth_req.into_bytes()).await;

        if let Err(write_err) = write_res {
            let msg = format!("Authenticating to server failed: {}", write_err);
            return Err(msg.into());
        }
    }

    // Wait for server to respond to auth request
    let auth_res = timeout(
        Duration::from_secs(5),
        handle_auth_response(tunnel_reader.clone()),
    )
    .await;

    match auth_res {
        Ok(res) => match res {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("Authentication to server failed: {}", e);
                error!("{}", msg);
                let mut writer = tunnel_writer.lock().await;
                let _ = writer.close().await;
                Err(msg.into())
            }
        },
        Err(_) => {
            let msg = "Server connection timeout";
            error!("{}", msg);
            let mut writer = tunnel_writer.lock().await;
            let _ = writer.close().await;
            Err(msg.into())
        }
    }
}

async fn handle_auth_response(tunnel: Arc<Mutex<TunnelReader>>) -> Result<()> {
    let mut client = tunnel.lock().await;

    // Authentication exchange should fit in 4k buffer
    // No need to accumulate the whole stream message
    let mut buffer = [0; 4096];

    info!("Waiting for server response...");

    match client.read(&mut buffer).await {
        Ok(0) => Err("Connection from client closed.".into()),
        Ok(n) => {
            // Strip off the EOF marker
            let request = TunnelMessage::from_buffer(&buffer[..n])?;
            if !request.is_auth_response() {
                return Err("Invalid tunnel auth response.".into());
            }

            if request.status_line.is_ok() {
                info!("Authentication to server successful.");
                return Ok(());
            }
            Err("Authentication to server failed.".into())
        }
        Err(e) => {
            let msg = format!("Failed to read from client stream: {}", e);
            error!("{}", msg);
            Err(msg.into())
        }
    }
}

async fn handle_requests(ctx: Arc<ClientContext>, tunnel: Arc<Mutex<TunnelReader>>) -> Result<()> {
    let mut client = tunnel.lock().await;
    let mut buffer = [0; 8192];

    // Accumulate stream data by looping over incoming message parts
    let mut tunnel_req: Option<TunnelMessage> = None;
    let client_error: Error;

    // Listen for all forward requests from the server
    // This look should only break if there are errors
    loop {
        let read_res = client.read(&mut buffer).await;

        match read_res {
            Ok(0) => {
                let error_msg = "No data received from server.";
                info!("{}", error_msg);
                client_error = error_msg.into();
                break;
            }
            Ok(n) => {
                // Debug body
                if let Some(mut res) = tunnel_req.take() {
                    // Try to complete the existing message first before accumulating more
                    let more_pos = res.accumulate_body(&buffer[..n]);
                    match more_pos {
                        Some(pos) => {
                            // Prev messages was completed
                            ctx.add_request(res).await;
                            tunnel_req = None;

                            // Parse more messages
                            let msg_res = TunnelMessage::from_large_buffer(&buffer[pos..n]);
                            match msg_res {
                                Ok(messages) => {
                                    for message in messages.into_iter() {
                                        if message.complete {
                                            ctx.add_request(message).await;
                                        } else {
                                            tunnel_req = Some(message);
                                        }
                                    }
                                }
                                Err(e) => {
                                    let msg =
                                        format!("Error reading back from connected client: {}", e);
                                    return Err(msg.into());
                                }
                            }
                        }
                        None => {
                            // No more messages
                            if res.complete {
                                ctx.add_request(res).await;
                                tunnel_req = None;
                            } else {
                                // Large body, read the next buffer
                                tunnel_req = Some(res);
                            }
                        }
                    }
                } else {
                    // This is a fresh buffer
                    let msg_res = TunnelMessage::from_large_buffer(&buffer[..n]);
                    match msg_res {
                        Ok(messages) => {
                            for message in messages.into_iter() {
                                if message.complete {
                                    ctx.add_request(message).await;
                                } else {
                                    tunnel_req = Some(message);
                                }
                            }
                        }
                        Err(e) => {
                            let msg = format!("Error reading back from server: {}", e);
                            return Err(msg.into());
                        }
                    }
                }
            }
            Err(e) => {
                let msg = format!("Failed to read from server stream: {}", e);
                return Err(msg.into());
            }
        }
    }

    Err(client_error)
}

async fn handle_forwards(
    ctx: Arc<ClientContext>,
    tunnel: Arc<Mutex<TunnelWriter>>,
    crawler: Client,
) -> Result<()> {
    loop {
        let maybe_req = ctx.get_request().await;

        if let Some(req) = maybe_req {
            let tunnel_clone = tunnel.clone();
            let config_clone = ctx.config.clone();
            let crawler_clone = crawler.clone();
            tokio::spawn(async move {
                handle_forward(tunnel_clone, config_clone, crawler_clone, req).await
            });
        }
    }

    Err("Forwarding loop exited.".into())
}

async fn handle_forward(
    tunnel: Arc<Mutex<TunnelWriter>>,
    config: Arc<ClientConfig>,
    crawler: Client,
    message: TunnelMessage,
) -> Result<()> {
    let res = handle_server_response(crawler, config, message).await?;
    if let Some(forward_res) = res {
        let mut client = tunnel.lock().await;
        if let Err(fwr_err) = client.write(&forward_res.into_bytes()).await {
            let msg = format!("Unable to send back response: {}", fwr_err);
            error!("{}", msg);
        }
    }

    Ok(())
}

async fn handle_server_response(
    crawler: Client,
    config: Arc<ClientConfig>,
    message: TunnelMessage,
) -> Result<Option<TunnelMessage>> {
    // Ignore all other types of messages
    if message.is_forward() {
        // Handle webhook requests
        let f_res = forward_request(crawler, config, message).await?;
        return Ok(Some(f_res));
    }

    Ok(None)
}

async fn forward_request(
    crawler: Client,
    config: Arc<ClientConfig>,
    message: TunnelMessage,
) -> Result<TunnelMessage> {
    let st_opt = match message.status_line {
        StatusLine::Request(req) => Some(req),
        _ => None,
    };
    let st = st_opt.expect("Message must be a forward request.");
    let method = st.method.as_str();
    let uri = st.path.as_str();

    let req_id = message.id.to_string();
    info!("Forwarding request: {} {} ID={}", method, uri, req_id);

    // Figure out the method
    // We assume that the target is a localhost address
    let protocol = match config.target_secure {
        true => "https",
        false => "http",
    };
    let url = format!("{}://{}{}", protocol, &config.target_address, uri);

    let mut r = crawler.request(ReqwestMethod::from_bytes(method.as_bytes()).unwrap(), url);

    // Inject headers
    for (k, v) in message.headers.iter() {
        // Skip some custom headers
        if k == WEBHOOK_OP || k == WEBHOOK_TOKEN {
            continue;
        }

        if k == "host" {
            // Rename host to the proxied target host
            r = r.header("host", &config.target_address);
        } else {
            r = r.header(k, v);
        }
    }

    if message.initial_body.len() > 0 {
        r = r.body(message.initial_body);
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

            let orig_id = message.id.clone();
            let mut tunnel_res = TunnelMessage::new(orig_id, status_line);
            tunnel_res.headers.extend(
                res.headers()
                    .iter()
                    .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap().to_string())),
            );

            // Mark this as a forward response
            tunnel_res
                .headers
                .push((WEBHOOK_OP.to_string(), WEBHOOK_OP_FORWARD_RES.to_string()));

            tunnel_res.initial_body = res.bytes().await.unwrap().to_vec();

            Ok(tunnel_res)
        }
        Err(e) => {
            let msg = format!("Forward request error: {}", e);
            error!(msg);
            Err(msg.into())
        }
    }
}
