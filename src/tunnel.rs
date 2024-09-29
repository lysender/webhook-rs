use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tracing::{error, info};

use crate::{config::ServerConfig, Error};
use crate::{token::verify_auth_token, Result};

pub struct TunnelClient {
    stream: Option<TcpStream>,
    verified: bool,
}

impl TunnelClient {
    pub fn new() -> Self {
        TunnelClient {
            stream: None,
            verified: false,
        }
    }

    pub fn with_stream(stream: TcpStream) -> Self {
        Self {
            stream: Some(stream),
            verified: false,
        }
    }

    pub fn verify(&mut self) {
        self.verified = true;
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    pub fn is_verified(&self) -> bool {
        self.verified && self.is_connected()
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if let Some(stream) = self.stream.as_mut() {
            return match stream.read(buf).await {
                Ok(n) => Ok(n),
                Err(e) => {
                    let msg = format!("Read client stream failed: {}", e);
                    Err(msg.into())
                }
            };
        }

        // No connection yet
        return Err("Read client stream failed: no client connection yet.".into());
    }

    pub async fn write(&mut self, data: &[u8]) -> Result<()> {
        if let Some(stream) = self.stream.as_mut() {
            return match stream.write_all(data).await {
                Ok(_) => Ok(()),
                Err(write_err) => {
                    let msg = format!("Write to client stream failed: {}", write_err);
                    Err(msg.into())
                }
            };
        }

        Err("Write to client stream failed: no client connection yet.".into())
    }
}

pub async fn start_tunnel_server(
    tunnel: Arc<Mutex<TunnelClient>>,
    config: Arc<ServerConfig>,
) -> Result<()> {
    let arc_config = config.clone();

    let address = format!("0.0.0.0:{}", arc_config.tunnel_port);
    let listener = TcpListener::bind(address.as_str()).await.unwrap();

    info!("Webhook tunnel server started at {}", address);

    loop {
        let tunnel_copy = tunnel.clone();
        let config_copy = arc_config.clone();

        // We only allow one client at a time, so whenever we have a new connection,
        // we just override the previous one.
        let res = listener.accept().await;
        match res {
            Ok((stream, addr)) => {
                info!("Connection established: {:?}", addr);
                tokio::spawn(handle_client(config_copy, tunnel_copy, stream));
            }
            Err(e) => {
                let mut client = tunnel_copy.lock().await;
                *client = TunnelClient::new();

                error!("Error accepting connection: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn handle_client(
    config: Arc<ServerConfig>,
    tunnel: Arc<Mutex<TunnelClient>>,
    stream: TcpStream,
) -> Result<()> {
    // Initialize stream connection but need to authenticate first
    {
        let mut client = tunnel.lock().await;
        *client = TunnelClient::with_stream(stream);
    }

    // Wait for client to authenticate
    let mut client = tunnel.lock().await;

    // This should be enough to verify auth requests
    let mut buffer = [0; 4096];
    match client.read(&mut buffer).await {
        Ok(0) => {
            // Connection closed
            info!("Connection from client closed.");
            return Ok(());
        }
        Ok(n) => {
            let message = String::from_utf8_lossy(&buffer[..n]).to_string();

            if valid_auth(message.as_str(), &config.jwt_secret).is_ok() {
                // Send response to client
                if let Err(reply_err) = client.write(b"WEBHOOK/1.0 200 OK\r\n").await {
                    error!("Sending OK reply failed: {}", reply_err);
                    return Ok(());
                } else {
                    info!("Client authenticated successfully.");
                    client.verify();
                }
            } else {
                info!("Client authentication failed.");

                // Send auth failed error to client
                if let Err(reply_err) = client.write(b"WEBHOOK/1.0 401 Unauthorized\r\n").await {
                    error!("Sending Unauthorized reply failed: {}", reply_err);
                    return Ok(());
                }
            }
        }
        Err(e) => {
            error!("Failed to read from client stream: {}", e);
            return Ok(());
        }
    }

    Ok(())
}

fn valid_auth(message: &str, secret: &str) -> Result<()> {
    // We expect 2 lines of data, the header command and the token header
    let lines: Vec<&str> = message.lines().collect();
    if lines.len() != 2 {
        return Err(Error::InvalidAuthToken);
    }

    if lines[0] == "AUTH /auth WEBHOOK/1.0" && lines[1].starts_with("Authorization: ") {
        if let Some(token) = lines[1].split(' ').last() {
            return verify_auth_token(token, secret);
        }
    }

    Err(Error::InvalidAuthToken)
}
