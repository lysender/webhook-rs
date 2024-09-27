use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tracing::{error, info};

use crate::config::ServerConfig;
use crate::Result;

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
    config: &ServerConfig,
) -> Result<()> {
    let address = format!("0.0.0.0:{}", config.tunnel_port);
    let listener = TcpListener::bind(address.as_str()).await.unwrap();

    info!("Webhook tunnel server started at {}", address);

    loop {
        let tunnel_copy = tunnel.clone();

        // We only allow one client at a time, so whenever we have a new connection,
        // we just override the previous one.
        let res = listener.accept().await;
        match res {
            Ok((stream, addr)) => {
                info!("Connection established: {:?}", addr);
                tokio::spawn(handle_client(tunnel_copy, stream));
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

async fn handle_client(tunnel: Arc<Mutex<TunnelClient>>, stream: TcpStream) -> Result<()> {
    // Initialize stream connection but need to authenticate first
    {
        let mut client = tunnel.lock().await;
        *client = TunnelClient::with_stream(stream);
    }

    // Wait for client to authenticate
    let mut client = tunnel.lock().await;

    // This should be enough to verify auth requests
    let mut buffer = [0; 1024];
    if let Ok(n) = client.read(&mut buffer).await {
        let message = String::from_utf8_lossy(&buffer[..n]).to_string();
        info!(message);

        if valid_auth(message) {
            // Send response to client
            if let Err(reply_err) = client.write(b"WEBHOOK/1.0 200 OK\n").await {
                error!("Sending OK reply failed: {}", reply_err);
                return Ok(());
            } else {
                info!("Client authenticated successfully.");
                client.verify();
            }
        } else {
            info!("Client authentication failed.");

            // Send auth failed error to client
            if let Err(reply_err) = client.write(b"WEBHOOK/1.0 401 Unauthorized\n").await {
                error!("Sending Unauthorized reply failed: {}", reply_err);
                return Ok(());
            }
        }
    }

    Ok(())
}

fn valid_auth(message: String) -> bool {
    // We expect 2 lines of data, the header command and the token
    let lines: Vec<&str> = message.lines().collect();
    if lines.len() != 2 {
        return false;
    }

    if lines[0] == "AUTH /auth WEBHOOK/1.0" {
        // Dummy authentication for now
        // TODO: Add authentication logic later
        return lines[1] == "Authorization: jwt_token";
    }

    return false;
}

fn valid_token(token: &str) -> bool {
    false
}
