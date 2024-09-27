use std::{
    io::{BufRead, BufReader, Write},
    thread,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};

use tracing::{error, info};

use crate::{config::ClientConfig, Result};

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
    match stream_res {
        Ok(stream) => {
            info!("Connected to server...");
            if let Err(conn_err) = handle_connection(stream).await {
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

async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let (mut reader, mut writer) = stream.split();

    info!("Authenticating to server...");

    // Before reading incoming messages, send a message to the server first
    let auth_msg = format!("AUTH /auth WEBHOOK/1.0\r\nAuthorization: jwt_token\n");
    let write_res = writer.write_all(auth_msg.as_bytes()).await;

    if let Err(write_err) = write_res {
        let msg = format!("Authenticating to server failed: {}", write_err);
        return Err(msg.into());
    }

    // Infinitely read data from the stream
    loop {
        info!("Reading data from stream...");
        // Check if we are properly authenticated
        let mut buffer = [0; 1024];
        if let Ok(n) = reader.read(&mut buffer).await {
            if n > 0 {
                // This may be just a partial body but may be enough for auth
                let message = String::from_utf8_lossy(&buffer[..n]).to_string();
                info!("Received message: {}", message);

                if let Some(first_line) = message.lines().next() {
                    if let Err(auth_err) = handle_auth_response(first_line).await {
                        return Err(auth_err);
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_auth_response(message: &str) -> Result<()> {
    match message {
        "WEBHOOK/1.0 200 OK" => Ok(()),
        "WEBHOOK/1.0 401 Unauthorized" => Err("Authentication failed".into()),
        _ => Ok(()),
    }
}
