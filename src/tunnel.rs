use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
    sync::Mutex,
    time::timeout,
};
use tracing::{error, info};

use crate::{
    config::ServerConfig,
    message::TunnelMessage,
    queue::{MessageMap, MessageQueue},
    Error,
};
use crate::{token::verify_auth_token, Result};

pub struct TunnelState {
    verified: Mutex<bool>,
}

pub struct TunnelReader {
    stream: Option<OwnedReadHalf>,
}

pub struct TunnelWriter {
    stream: Option<OwnedWriteHalf>,
}

impl TunnelState {
    pub fn new() -> Self {
        Self {
            verified: Mutex::new(false),
        }
    }

    pub async fn is_verified(&self) -> bool {
        let verified = self.verified.lock().await;
        *verified
    }

    pub async fn verify(&self) {
        let mut verified = self.verified.lock().await;
        *verified = true;
    }
}

impl TunnelReader {
    pub fn new(stream: OwnedReadHalf) -> Self {
        Self {
            stream: Some(stream),
        }
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
        return Err("Stream reader failed: no client connection yet.".into());
    }
}

impl TunnelWriter {
    pub fn new(stream: OwnedWriteHalf) -> Self {
        Self {
            stream: Some(stream),
        }
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
                let msg = format!("Failed to shutdown client writer stream: {}", shutdown_err);
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
    config: Arc<ServerConfig>,
    tunnel_state: Arc<TunnelState>,
    req_queue: Arc<MessageQueue>,
    res_map: Arc<MessageMap>,
) -> Result<()> {
    let arc_config = config.clone();

    let address = format!("0.0.0.0:{}", arc_config.tunnel_port);
    let listener = TcpListener::bind(address.as_str()).await.unwrap();

    info!("Webhook tunnel server started at {}", address);

    loop {
        info!("Waiting for incoming connections...");

        // Will keep accepting new connections
        let res = listener.accept().await;

        // Only accept one client
        match res {
            Ok((stream, addr)) => {
                info!("Connection established: {}", addr);
                let client_res = handle_client(
                    arc_config.clone(),
                    tunnel_state.clone(),
                    stream,
                    req_queue.clone(),
                    res_map.clone(),
                )
                .await;
                if let Err(e) = client_res {
                    error!("Error handling client: {}", e);
                }
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn handle_client(
    config: Arc<ServerConfig>,
    tunnel_state: Arc<TunnelState>,
    stream: TcpStream,
    req_queue: Arc<MessageQueue>,
    res_map: Arc<MessageMap>,
) -> Result<()> {
    // Split stream
    let (reader, writer) = stream.into_split();
    let tunnel_reader = Arc::new(Mutex::new(TunnelReader::new(reader)));
    let tunnel_writer = Arc::new(Mutex::new(TunnelWriter::new(writer)));

    let _ = authenticate(config.clone(), tunnel_reader.clone(), tunnel_writer.clone()).await?;

    tunnel_state.verify().await;

    // Once authenticated, spawn two tasks:
    // 1. Read request queue and write them to client stream
    // 2. Read client stream and write to response map

    let join_res = tokio::try_join!(
        handle_requests(tunnel_writer, req_queue),
        handle_responses(tunnel_reader, res_map)
    );

    if let Err(e) = join_res {
        let msg = format!("{}", e);
        return Err(msg.into());
    }

    Ok(())
}

async fn authenticate(
    config: Arc<ServerConfig>,
    tunnel_reader: Arc<Mutex<TunnelReader>>,
    tunnel_writer: Arc<Mutex<TunnelWriter>>,
) -> Result<()> {
    // Wait for client to authenticate
    match timeout(
        Duration::from_secs(5),
        handle_auth(config, tunnel_reader.clone(), tunnel_writer.clone()),
    )
    .await
    {
        Ok(res) => match res {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("Client authentication failed: {}", e);
                error!("{}", msg);
                let mut client = tunnel_writer.lock().await;
                let _ = client.close().await;
                Err(msg.into())
            }
        },
        Err(_) => {
            let msg = "Client connection timeout";
            error!("{}", msg);
            let mut client = tunnel_writer.lock().await;
            let _ = client.close().await;
            Err(msg.into())
        }
    }
}

async fn handle_auth(
    config: Arc<ServerConfig>,
    tunnel_reader: Arc<Mutex<TunnelReader>>,
    tunnel_writer: Arc<Mutex<TunnelWriter>>,
) -> Result<()> {
    let mut client = tunnel_reader.lock().await;

    // We don't need a large buffer nor need to read the whole stream
    // as we will only process auth headers here and ignore other requests...
    let mut buffer = [0; 4096];

    // At this point, we only expect the client to send an auth request
    match client.read(&mut buffer).await {
        Ok(0) => Err("Connection from client closed.".into()),
        Ok(n) => {
            // Strip off the EOF marker
            let request = TunnelMessage::from_buffer(&buffer[..n])?;
            if !request.is_auth() {
                return Err("Invalid tunnel auth request.".into());
            }

            if valid_auth(&request, &config.jwt_secret).is_ok() {
                // Send response to client
                let orig_id = request.id.clone();
                let ok_msg = TunnelMessage::with_auth_ok(orig_id);

                let mut writer = tunnel_writer.lock().await;
                if let Err(reply_err) = writer.write(&ok_msg.into_bytes()).await {
                    let msg = format!("Sending OK reply failed: {}", reply_err);
                    return Err(msg.into());
                }

                info!("Client authenticated successfully.");
                return Ok(());
            } else {
                error!("Invalid authorization code.");

                // Send auth failed error to client
                let orig_id = request.id.clone();
                let err_msg = TunnelMessage::with_auth_unauthorized(orig_id);

                let mut writer = tunnel_writer.lock().await;
                if let Err(reply_err) = writer.write(&err_msg.into_bytes()).await {
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

async fn handle_requests(
    tunnel_writer: Arc<Mutex<TunnelWriter>>,
    req_queue: Arc<MessageQueue>,
) -> Result<()> {
    loop {
        let message = req_queue.pop().await;
        if let Some(msg) = message {
            let writer_clone = tunnel_writer.clone();
            tokio::spawn(async move {
                let mut client = writer_clone.lock().await;
                if let Err(write_err) = client.write(&msg.into_bytes()).await {
                    error!("Failed to write to client stream: {}", write_err);
                }
            });
        }
    }

    Ok(())
}

async fn handle_responses(
    tunnel_reader: Arc<Mutex<TunnelReader>>,
    res_queue: Arc<MessageMap>,
) -> Result<()> {
    let mut client = tunnel_reader.lock().await;

    // Read from client response
    let mut buffer = [0; 8192];
    let mut tunnel_res: Option<TunnelMessage> = None;

    loop {
        match client.read(&mut buffer).await {
            Ok(0) => {
                return Err("No response from connected client".into());
            }
            Ok(n) => {
                if let Some(mut res) = tunnel_res.take() {
                    // Append data to existing body, assuming these are part of the data
                    if res.accumulate_body(&buffer[..n]) {
                        // Body complete, push to response queue
                        res_queue.add(res).await;

                        tunnel_res = None;
                    } else {
                        // Continue accumulating body
                        tunnel_res = Some(res);
                    }
                } else {
                    // This is a fresh buffer, read headers
                    let fresh_buffer = TunnelMessage::from_buffer(&buffer[..n]);
                    match fresh_buffer {
                        Ok(res) => {
                            if res.complete {
                                // Add to queue and continue reading for new responses
                                res_queue.add(res).await;
                                tunnel_res = None;
                            } else {
                                tunnel_res = Some(res);
                            }
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    };
                }
            }
            Err(e) => {
                let msg = format!("Error reading back from connected client: {}", e);
                return Err(msg.into());
            }
        };
    }
}
