use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    time::timeout,
};
use tracing::{error, info};

use crate::{
    config::ServerConfig,
    message::{len_without_eof_marker, TunnelMessage},
    Error,
};
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

    pub async fn close(&mut self) -> Result<()> {
        info!("Closing TCP connection from client.");

        if let Some(stream) = self.stream.as_mut() {
            if let Err(shutdown_err) = stream.shutdown().await {
                // We really need to close the stream so we let the error pass
                let msg = format!("Failed to shutdown client stream: {}", shutdown_err);
                error!(msg);
                self.stream = None;

                return Err(msg.into());
            }
        }

        self.stream = None;
        Ok(())
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
        info!("Waiting for incoming connections...");

        let tunnel_copy = tunnel.clone();
        let config_copy = arc_config.clone();

        // Will keep accepting new connections
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
    match timeout(Duration::from_secs(5), handle_auth(config, tunnel.clone())).await {
        Ok(res) => match res {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("Client authentication failed: {}", e);
                error!("{}", msg);
                let mut client = tunnel.lock().await;
                let _ = client.close().await;
                Err(msg.into())
            }
        },
        Err(_) => {
            let msg = "Client connection timeout";
            error!("{}", msg);
            let mut client = tunnel.lock().await;
            let _ = client.close().await;
            Err(msg.into())
        }
    }
}

async fn handle_auth(config: Arc<ServerConfig>, tunnel: Arc<Mutex<TunnelClient>>) -> Result<()> {
    let mut client = tunnel.lock().await;

    // We don't need a large buffer nor need to read the whole stream
    // as we will only process auth headers here and ignore other requests...
    let mut buffer = [0; 4096];

    // At this point, we only expect the client to send an auth request
    match client.read(&mut buffer).await {
        Ok(0) => Err("Connection from client closed.".into()),
        Ok(n) => {
            // Strip off the EOF marker
            let buflen = len_without_eof_marker(&buffer, n).unwrap_or(n);
            let request = TunnelMessage::from_buffer(&buffer[..buflen])?;
            if !request.is_auth() {
                return Err("Invalid tunnel auth request.".into());
            }

            if valid_auth(&request, &config.jwt_secret).is_ok() {
                // Send response to client
                let ok_msg = TunnelMessage::with_auth_ok();

                if let Err(reply_err) = client.write(&ok_msg.into_bytes()).await {
                    let msg = format!("Sending OK reply failed: {}", reply_err);
                    return Err(msg.into());
                }

                info!("Client authenticated successfully.");
                client.verify();
                return Ok(());
            } else {
                error!("Invalid authorization code.");

                // Send auth failed error to client
                let err_msg = TunnelMessage::with_auth_unauthorized();

                if let Err(reply_err) = client.write(&err_msg.into_bytes()).await {
                    let msg = format!("Sending Unauthorized reply failed: {}", reply_err);
                    return Err(msg.into());
                }

                return Err("Invalid authorization code.".into());
            }
        }
        Err(e) => {
            let msg = format!("Failed to read from client stream: {}", e);
            Err(msg.into())
        }
    }
}

fn valid_auth(request: &TunnelMessage, secret: &str) -> Result<()> {
    // Find the auth token
    match request.webhook_token() {
        Some(token) => verify_auth_token(token, secret),
        None => Err(Error::InvalidAuthToken),
    }
}
