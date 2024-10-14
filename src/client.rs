use reqwest::Client;
use reqwest::Method as ReqwestMethod;
use std::sync::Arc;
use std::{thread, time::Duration};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

use tracing::{error, info};

use crate::message::ResponseLine;
use crate::message::StatusLine;
use crate::message::TunnelMessage;
use crate::message::WEBHOOK_OP;
use crate::message::WEBHOOK_OP_FORWARD_RES;
use crate::message::WEBHOOK_TOKEN;
use crate::queue::MessageQueue;
use crate::tunnel::TunnelReader;
use crate::tunnel::TunnelWriter;
use crate::Error;
use crate::{config::ClientConfig, token::create_auth_token, Result};

pub async fn start_client(config: &ClientConfig) {
    loop {
        let con = { connect(&config).await };

        if let Err(e) = con {
            error!("Connection error: {}", e);
            info!("Reconnecting in 10 seconds...");

            thread::sleep(Duration::from_secs(10));
        }
    }
}

async fn connect(config: &ClientConfig) -> Result<()> {
    let stream_res = TcpStream::connect(&config.tunnel_address).await;
    let crawler = Client::new();

    match stream_res {
        Ok(stream) => {
            info!("Connected to server...");
            handle_connection(crawler, config, stream).await
        }
        Err(e) => {
            let connect_err = format!("Error connecting to the server: {}", e);
            Err(connect_err.into())
        }
    }
}

async fn handle_connection(
    crawler: Client,
    config: &ClientConfig,
    stream: TcpStream,
) -> Result<()> {
    info!("Authenticating to server...");

    let (reader, writer) = stream.into_split();
    let tunnel_reader = Arc::new(Mutex::new(TunnelReader::new(reader)));
    let tunnel_writer = Arc::new(Mutex::new(TunnelWriter::new(writer)));

    let _ = authenticate(tunnel_reader.clone(), tunnel_writer.clone(), config).await?;

    let req_queue = Arc::new(MessageQueue::new());

    let join_res = tokio::try_join!(
        handle_requests(tunnel_reader, req_queue.clone()),
        handle_forwards(tunnel_writer, req_queue, &config, crawler)
    );

    if let Err(e) = join_res {
        let msg = format!("{}", e);
        return Err(msg.into());
    }

    Err("Connection closed.".into())
}

async fn authenticate(
    tunnel_reader: Arc<Mutex<TunnelReader>>,
    tunnel_writer: Arc<Mutex<TunnelWriter>>,
    config: &ClientConfig,
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

async fn handle_requests(
    tunnel: Arc<Mutex<TunnelReader>>,
    req_queue: Arc<MessageQueue>,
) -> Result<()> {
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
                            req_queue.push(res).await;
                            tunnel_req = None;

                            // Parse more messages
                            let msg_res = TunnelMessage::from_large_buffer(&buffer[pos..n]);
                            match msg_res {
                                Ok(messages) => {
                                    for message in messages.into_iter() {
                                        if message.complete {
                                            req_queue.push(message).await;
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
                                req_queue.push(res).await;
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
                                    req_queue.push(message).await;
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
    tunnel: Arc<Mutex<TunnelWriter>>,
    req_queue: Arc<MessageQueue>,
    config: &ClientConfig,
    crawler: Client,
) -> Result<()> {
    loop {
        let maybe_req = req_queue.pop().await;

        if let Some(req) = maybe_req {
            let tunnel_clone = tunnel.clone();
            let config_clone = config.clone();
            let crawler_clone = crawler.clone();
            tokio::spawn(async move {
                handle_forward(tunnel_clone, &config_clone, crawler_clone, req).await
            });
        }
    }

    Err("Forwarding loop exited.".into())
}

async fn handle_forward(
    tunnel: Arc<Mutex<TunnelWriter>>,
    config: &ClientConfig,
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
    config: &ClientConfig,
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
    config: &ClientConfig,
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
