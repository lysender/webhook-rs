use reqwest::Client;
use reqwest::Method as ReqwestMethod;
use std::sync::Arc;
use std::{thread, time::Duration};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use tracing::{error, info};

use crate::parser::len_without_eof_marker;
use crate::parser::ResponseLine;
use crate::parser::StatusLine;
use crate::parser::TunnelMessage;
use crate::parser::WEBHOOK_OP_FORWARD_RES;
use crate::parser::X_WEEB_HOOK_OP;
use crate::parser::X_WEEB_HOOK_TOKEN;
use crate::tunnel::TunnelClient;
use crate::Error;
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

    handle_forward_requests(tunnel, config, crawler).await
}

async fn authenticate(tunnel: Arc<Mutex<TunnelClient>>, config: &ClientConfig) -> Result<()> {
    let token = create_auth_token(&config.jwt_secret)?;
    let auth_req = TunnelMessage::with_auth_token(token);

    let mut client = tunnel.lock().await;
    let write_res = client.write(&auth_req.into_bytes()).await;

    if let Err(write_err) = write_res {
        let msg = format!("Authenticating to server failed: {}", write_err);
        return Err(msg.into());
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

    match client.read(&mut buffer).await {
        Ok(0) => Err("Connection from client closed.".into()),
        Ok(n) => {
            // Strip off the EOF marker
            let buflen = len_without_eof_marker(&buffer, n).unwrap_or(n);
            let request = TunnelMessage::from_buffer(&buffer[..buflen])?;
            if !request.is_auth_response() {
                return Err("Invalid tunnel auth response.".into());
            }

            if request.status_line.is_ok() {
                return Ok(());
            }
            Err("Authentication to server failed.".into())
        }
        Err(e) => {
            let msg = format!("Failed to read from client stream: {}", e);
            Err(msg.into())
        }
    }
}

async fn handle_forward_requests(
    tunnel: Arc<Mutex<TunnelClient>>,
    config: &ClientConfig,
    crawler: Client,
) -> Result<()> {
    let mut client = tunnel.lock().await;

    let mut buffer = [0; 4096];

    // Accumulate stream data by looping over incoming message parts
    let mut tunnel_req: Option<TunnelMessage> = None;

    // Listen for all forward requests from the server
    loop {
        match client.read(&mut buffer).await {
            Ok(0) => {
                info!("No data received from server.");
                break;
            }
            Ok(n) => {
                if let Some(mut res) = tunnel_req.take() {
                    info!("Appending data to existing request.");
                    // Append data to existing body, assuming these are part of the data
                    let mut buflen = n;
                    let mut complete = false;
                    if let Some(adjusted_len) = len_without_eof_marker(&buffer, n) {
                        buflen = adjusted_len;
                        complete = true;
                    }
                    res.initial_body.extend_from_slice(&buffer[..buflen]);

                    if complete {
                        let handled_res =
                            handle_server_response(crawler.clone(), config, res).await?;
                        if let Some(forward_res) = handled_res {
                            if let Err(fwr_err) = client.write(&forward_res.into_bytes()).await {
                                let msg = format!("Unable to send back response: {}", fwr_err);
                                return Err(msg.into());
                            }
                        }
                        // Clear the current request
                        tunnel_req = None;
                    }
                } else {
                    // This is a fresh buffer, read headers
                    let mut buflen = n;
                    let mut complete = false;
                    if let Some(adjusted_len) = len_without_eof_marker(&buffer, n) {
                        buflen = adjusted_len;
                        complete = true;
                    }

                    let fresh_buffer = TunnelMessage::from_buffer(&buffer[..buflen]);
                    match fresh_buffer {
                        Ok(res) => {
                            info!("Received response from connected client.");

                            if complete {
                                let handled_res =
                                    handle_server_response(crawler.clone(), config, res).await?;
                                if let Some(forward_res) = handled_res {
                                    if let Err(fwr_err) =
                                        client.write(&forward_res.into_bytes()).await
                                    {
                                        let msg =
                                            format!("Unable to send back response: {}", fwr_err);
                                        return Err(msg.into());
                                    }
                                }
                                // Clear the current request
                                tunnel_req = None;
                                break;
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

                info!("Waiting for next message...");
            }
            Err(e) => {
                let msg = format!("Failed to read from server stream: {}", e);
                return Err(msg.into());
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
    if message.is_forward() {
        // Handle webhook requests
        let f_res = forward_request(crawler, config, message).await?;
        // Return the response from target app if there are any
        return Ok(Some(f_res));
    } else if message.is_auth_response() {
        if message.status_line.is_ok() {
            info!("Authentication to server successful.");
            return Ok(None);
        } else {
            error!("Authentication to server failed.");
            return Err("Authentication to server failed.".into());
        }
    } else {
        let msg = format!("Unsupported tunnel message");
        error!(msg);
        return Err(msg.into());
    }
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
    let Some(st) = st_opt else {
        let msg = format!("Invalid tunnel message");
        error!(msg);
        return Err(msg.into());
    };

    let method = st.method.as_str();
    let uri = st.path.as_str();

    info!("Forwarding request: {} {}", method, uri);

    // Figure out the method
    // We assume that the target is a localhost address
    let url = format!(
        "http://{}:{}{}",
        &config.target_host, &config.target_port, uri
    );

    let mut r = crawler.request(ReqwestMethod::from_bytes(method.as_bytes()).unwrap(), url);

    // Inject headers
    for (k, v) in message.headers.iter() {
        // Skip some custom headers
        if k == X_WEEB_HOOK_OP || k == X_WEEB_HOOK_TOKEN {
            continue;
        }

        if k == "host" {
            // Rename host
            r = r.header("host", &config.target_host);
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

            let mut tunnel_res = TunnelMessage::new(status_line);
            tunnel_res.headers = res
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap().to_string()))
                .collect();

            // Mark this as a forward response
            tunnel_res.headers.push((
                X_WEEB_HOOK_OP.to_string(),
                WEBHOOK_OP_FORWARD_RES.to_string(),
            ));

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
