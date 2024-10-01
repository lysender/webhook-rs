use reqwest::Client;
use reqwest::Method as ReqwestMethod;
use std::{thread, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use tracing::{error, info};

use crate::parser::StatusLine;
use crate::parser::TunnelMessage;
use crate::{config::ClientConfig, token::create_auth_token, Result};

pub async fn start_client(config: &ClientConfig) {
    loop {
        let conn = connect(&config).await;
        if let Err(e) = conn {
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
            if let Err(conn_err) = handle_connection(crawler, config, stream).await {
                return Err(conn_err);
            }
            Ok(())
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
    mut stream: TcpStream,
) -> Result<()> {
    info!("Authenticating to server...");

    // Before reading incoming messages, authenticate to the server first
    let token = create_auth_token(&config.jwt_secret)?;
    let auth_req = TunnelMessage::with_tunnel_auth(token);
    let write_res = stream.write_all(&auth_req.into_bytes()).await;

    if let Err(write_err) = write_res {
        let msg = format!("Authenticating to server failed: {}", write_err);
        return Err(msg.into());
    }

    // Authentication exchange should fit in 4k buffer
    let mut buffer = [0; 4096];

    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                // No data to read, so we treat this as invalid connection
                info!("No data received from server.");
                break;
            }
            Ok(n) => {
                // We got filled, let's read it
                let res = TunnelMessage::from_buffer(&buffer[..n])?;
                let handled_res = handle_server_response(crawler.clone(), config, res).await?;
                if let Some(forward_res) = handled_res {
                    if let Err(fwr_err) = stream.write_all(forward_res.as_bytes()).await {
                        let msg = format!("Unable to send back response: {}", fwr_err);
                        return Err(msg.into());
                    }
                }

                info!("Waiting for next message...");
            }
            Err(e) => {
                let msg = format!("Failed to read from server stream: {}", e);
                return Err(msg.into());
            }
        };
    }

    Ok(())
}

async fn handle_server_response(
    crawler: Client,
    config: &ClientConfig,
    message: TunnelMessage,
) -> Result<Option<String>> {
    match &message.status_line {
        StatusLine::TunnelRequest(req) => {
            // Handle tunnel requests
            if req.method.as_str() == "FORWARD" {
                // Handle webhook requests
                let f_res = forward_request(crawler, config, &message.initial_body).await?;
                // Return the response from target app if there are any
                return Ok(Some(f_res));
            }

            Ok(None)
        }
        StatusLine::TunnelResponse(res) => {
            // Handle tunnel responses
            match res.status_code {
                200 => {
                    info!("Authentication to server successful.");
                    return Ok(None);
                }
                401 => {
                    error!("Authentication to server failed.");
                    return Err("Authentication to server failed.".into());
                }
                _ => {
                    let msg = format!("Unrecognized status code: {}", res.status_code);
                    error!(msg);
                    return Err(msg.into());
                }
            }
        }
        _ => {
            let msg = format!("Unsupported tunnel message");
            error!(msg);
            return Err(msg.into());
        }
    }
}

async fn forward_request(crawler: Client, config: &ClientConfig, buffer: &[u8]) -> Result<String> {
    let message = TunnelMessage::from_buffer(buffer)?;
    let st_opt = match &message.status_line {
        StatusLine::HttpRequest(s) => Some(s),
        _ => None,
    };

    let Some(st) = st_opt else {
        return Err("Invalid HTTP request payload.".into());
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
        // Inject headers
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
            // Build the whole response back into string
            let status_line = format!("{:?} {}\r\n", res.version(), res.status());
            let headers = res
                .headers()
                .iter()
                .map(|(k, v)| format!("{}: {}\r\n", k, v.to_str().unwrap()))
                .collect::<String>();

            let body = res.text().await.unwrap();
            let full_response = format!("{}{}\r\n{}", status_line, headers, body);

            Ok(full_response)
        }
        Err(e) => {
            let msg = format!("Forward request error: {}", e);
            error!(msg);
            Err(msg.into())
        }
    }
}
