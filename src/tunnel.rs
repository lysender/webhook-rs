use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, Duration},
};
use tracing::{error, info};

use crate::client::TunnelClient;
use crate::{config::Config, Result};

pub async fn start_tunnel_server(tunnel: Arc<Mutex<TunnelClient>>, config: &Config) -> Result<()> {
    info!("Starting tunnel server...");
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
            if let Err(reply_err) = client.write(b"WEBHOOK/1.0 AUTH-OK\n").await {
                error!("Sending AUTH-OK reply failed: {}", reply_err);
                return Ok(());
            } else {
                info!("Client authenticated successfully.");
                client.verify();
            }
        }
    }

    //loop {
    //    info!("Sending PING to client...");
    //    if let Err(e) = stream.write_all(b"PING\n").await {
    //        error!("Error writing to stream: {:?}", e);
    //        break;
    //    }
    //
    //    sleep(Duration::from_secs(60)).await;
    //}

    Ok(())
}

fn valid_auth(message: String) -> bool {
    // We expect 2 lines of data, the header command and the token
    let lines: Vec<&str> = message.lines().collect();
    if lines.len() != 2 {
        return false;
    }

    if lines[0] == "WEBHOOK/1.0 AUTH" {
        // Dummy authentication for now
        // TODO: Add authentication logic later
        return lines[1] == "jwt_token";
    }

    return false;
}
