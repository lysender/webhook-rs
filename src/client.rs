use reqwest::Client;
use reqwest::Method as ReqwestMethod;
use std::{thread, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use tracing::{error, info};

use crate::{config::ClientConfig, token::create_auth_token, Result};

pub async fn start_client(config: &ClientConfig) {
    loop {
        let conn = connect(&config).await;
        if let Err(e) = conn {
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

    // Before reading incoming messages, send a message to the server first
    let token = create_auth_token(&config.jwt_secret)?;
    let auth_msg = format!("AUTH /auth WEBHOOK/1.0\r\nAuthorization: {}\n", token);
    let write_res = stream.write_all(auth_msg.as_bytes()).await;

    if let Err(write_err) = write_res {
        let msg = format!("Authenticating to server failed: {}", write_err);
        return Err(msg.into());
    }

    // Try to read messages with a large buffer first, let's see what happens
    let mut buffer = [0; 4096];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                // End of line
                error!("Server closed the connection.");
                break;
            }
            Ok(n) => {
                // We got filled, let's read it
                let message = String::from_utf8_lossy(&buffer[..n]).to_string();
                println!(">>>{}<<<", message);

                let handled_res =
                    handle_server_response(crawler.clone(), config, message.as_str()).await?;
                if let Some(forward_res) = handled_res {
                    if let Err(fwr_err) = stream.write_all(forward_res.as_bytes()).await {
                        let msg = format!("Unable to send back response: {}", fwr_err);
                        return Err(msg.into());
                    }
                }
            }
            Err(e) => {
                error!("Failed to read from server stream: {}", e);
                return Ok(());
            }
        }
    }

    Ok(())
}

async fn handle_server_response(
    crawler: Client,
    config: &ClientConfig,
    message: &str,
) -> Result<Option<String>> {
    // Get the request line
    if let Some(first_pos) = message.find("\r\n") {
        let (request_line, remaining) = message.split_at(first_pos);

        println!("Req line: {}", request_line);
        println!("Remaining: {}", remaining);

        match request_line {
            "WEBHOOK/1.0 200 OK" => {
                info!("Authentication to server successful.");
                return Ok(None);
            }
            "WEBHOOK/1.0 401 Unauthorized" => {
                error!("Authentication to server failed.");
                return Err("Authentication to server failed.".into());
            }
            "FORWARD /webhook WEBHOOK/1.0" => {
                info!("Forward request received...");

                // Do the forwarding here...
                // Strip the next empty line then forward the rest
                let remaining_data = &remaining[4..];
                let f_res = forward_request(crawler, config, remaining_data).await?;
                // Return the response from target app if there are any
                return Ok(Some(f_res));
            }
            _ => {
                let msg = format!("Unrecognized request line: {}", request_line);
                error!(msg);
                return Err(msg.into());
            }
        }
    }

    error!("No request line found.");
    Err("No request line found.".into())
}

async fn forward_request(crawler: Client, config: &ClientConfig, message: &str) -> Result<String> {
    // We actually need to parse the message, my goodness
    let mut rn_lines = message.split("\r\n");

    let request_line = rn_lines.next().unwrap();
    // Figure out the method
    let mut rl_parts = request_line.split_whitespace();
    let method = rl_parts.next().unwrap();
    let uri = rl_parts.next().unwrap();

    // We assume that the target is a localhost address
    let url = format!(
        "http://{}:{}{}",
        &config.target_host, &config.target_port, uri
    );

    let mut r = crawler.request(ReqwestMethod::from_bytes(method.as_bytes()).unwrap(), url);

    let mut is_body = false;
    let mut body: Vec<u8> = Vec::new();

    // Inject headers
    for header_line in rn_lines {
        if header_line == "" {
            // Could be the end of headers
            // body next...
            is_body = true;
            continue;
        }

        if is_body {
            let line = header_line.to_string();
            body.extend_from_slice(&line.as_bytes());
        } else {
            // Inject headers
            let mut h_parts = header_line.split(": ");
            let h_k = h_parts.next().unwrap();
            let h_v = h_parts.next().unwrap();

            if h_k == "host" {
                // Rename host
                r = r.header("host", &config.target_host);
            } else {
                r = r.header(h_k, h_v);
            }
        }
    }

    if body.len() > 0 {
        r = r.body(body);
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
