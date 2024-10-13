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
use crate::tunnel::TunnelClient;
use crate::{config::ClientConfig, token::create_auth_token, Result};

pub async fn start_client(config: &ClientConfig) {
    loop {
        if let Err(e) = connect(&config).await {
            error!("Connection error: {}", e);
            info!("Reconnecting in 10 seconds...");

            thread::sleep(Duration::from_secs(10));
        }

        info!("Going into a loop...");
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

    let tunnel = Arc::new(Mutex::new(TunnelClient::with_stream(stream)));
    let _ = authenticate(tunnel.clone(), config).await?;

    {
        let mut client = tunnel.lock().await;
        client.verify();
    }

    let req_queue = Arc::new(MessageQueue::new());
    // handle_messages(tunnel, req_queue.clone(), config, crawler).await

    let tunnel_clone = tunnel.clone();
    let req_queue_clone = req_queue.clone();
    let req_task =
        tokio::spawn(async move { handle_requests(tunnel_clone, req_queue_clone).await });

    let config_clone = config.clone();
    let forward_task =
        tokio::spawn(
            async move { handle_forwards(tunnel, req_queue, &config_clone, crawler).await },
        );

    let _ = tokio::try_join!(req_task, forward_task);

    Ok(())
}

async fn authenticate(tunnel: Arc<Mutex<TunnelClient>>, config: &ClientConfig) -> Result<()> {
    let id = Uuid::now_v7();
    let token = create_auth_token(&config.jwt_secret)?;
    let auth_req = TunnelMessage::with_auth_token(id, token);

    {
        let mut client = tunnel.lock().await;
        let write_res = client.write(&auth_req.into_bytes()).await;

        if let Err(write_err) = write_res {
            let msg = format!("Authenticating to server failed: {}", write_err);
            return Err(msg.into());
        }
    }

    // Wait for server to respond to auth request
    match timeout(Duration::from_secs(5), handle_auth_response(tunnel.clone())).await {
        Ok(res) => match res {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("Authentication to server failed: {}", e);
                error!("{}", msg);
                let mut client = tunnel.lock().await;
                let _ = client.close().await;
                Err(msg.into())
            }
        },
        Err(_) => {
            let msg = "Server connection timeout";
            error!("{}", msg);
            let mut client = tunnel.lock().await;
            let _ = client.close().await;
            Err(msg.into())
        }
    }
}

async fn handle_auth_response(tunnel: Arc<Mutex<TunnelClient>>) -> Result<()> {
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
    tunnel: Arc<Mutex<TunnelClient>>,
    req_queue: Arc<MessageQueue>,
) -> Result<()> {
    let mut buffer = [0; 8192];

    // Accumulate stream data by looping over incoming message parts
    let mut tunnel_req: Option<TunnelMessage> = None;

    // Listen for all forward requests from the server
    // This look should only break if there are errors
    loop {
        println!("Waiting for messages from server...");
        let read_res = {
            let mut client = tunnel.lock().await;
            client.read(&mut buffer).await
        };

        match read_res {
            Ok(0) => {
                info!("No data received from server.");
                break;
            }
            Ok(n) => {
                if let Some(mut res) = tunnel_req.take() {
                    let complete = res.accumulate_body(&buffer[..n]);
                    if complete {
                        let rq = req_queue.clone();
                        rq.push(res).await;

                        // Clear the current request
                        tunnel_req = None;
                    } else {
                        // Continue accumulating
                        tunnel_req = Some(res);
                    }
                } else {
                    // This is a fresh buffer, read headers
                    let fresh_buffer = TunnelMessage::from_buffer(&buffer[..n]);
                    match fresh_buffer {
                        Ok(res) => {
                            if res.complete {
                                let rq = req_queue.clone();
                                rq.push(res).await;

                                // Clear the current request
                                tunnel_req = None;
                            } else {
                                tunnel_req = Some(res);
                            }
                        }
                        Err(e) => {
                            let msg = format!("Error reading back from connected client: {}", e);
                            return Err(msg.into());
                        }
                    };
                }
            }
            Err(e) => {
                let msg = format!("Failed to read from server stream: {}", e);
                return Err(msg.into());
            }
        }
    }

    Ok(())
}

async fn handle_forwards(
    tunnel: Arc<Mutex<TunnelClient>>,
    req_queue: Arc<MessageQueue>,
    config: &ClientConfig,
    crawler: Client,
) -> Result<()> {
    loop {
        println!("Waiting for forward messages from queue...");
        let maybe_req = {
            let rq = req_queue.clone();
            rq.pop().await
        };

        if let Some(req) = maybe_req {
            println!("Got some forward request from queue...");
            let res = handle_server_response(crawler.clone(), config, req).await?;
            if let Some(forward_res) = res {
                let mut client = tunnel.lock().await;
                if let Err(fwr_err) = client.write(&forward_res.into_bytes()).await {
                    let msg = format!("Unable to send back response: {}", fwr_err);
                    return Err(msg.into());
                }
            }
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

    info!("Forwarding request: {} {}", method, uri);

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
