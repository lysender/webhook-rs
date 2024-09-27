use std::{thread, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};

use tracing::{error, info};

use crate::{config::ClientConfig, Error, Result};

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

    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();

        match buf_reader.read_line(&mut line).await {
            Ok(0) => {
                // Connection closed
                break;
            }
            Ok(_) => {
                // Received some message
                let msg = line.trim();
                info!("Received message: {}", msg);
                if let Err(auth_err) = handle_auth_response(msg).await {
                    return Err(auth_err);
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

async fn handle_auth_response(message: &str) -> Result<()> {
    match message {
        "WEBHOOK/1.0 200 OK" => Ok(()),
        "WEBHOOK/1.0 401 Unauthorized" => Err("Authentication failed".into()),
        _ => Ok(()),
    }
}
