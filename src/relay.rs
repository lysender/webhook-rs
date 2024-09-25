use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, Duration},
};
use tracing::{error, info};

use crate::{config::Config, Result};

pub async fn start_relay_server(
    client: Arc<Mutex<Option<TcpStream>>>,
    config: &Config,
) -> Result<()> {
    info!("Starting relay server...");
    let address = format!("0.0.0.0:{}", config.tunnel_port);
    let listener = TcpListener::bind(address.as_str()).await.unwrap();

    info!("Webhook relay server started at {}", address);

    loop {
        let client_copy = client.clone();

        // We only allow one client at a time, so whenever we have a new connection,
        // we just override the previous one.
        let res = listener.accept().await;
        match res {
            Ok((stream, addr)) => {
                info!("Connection established: {:?}", addr);
                tokio::spawn(handle_client(client_copy, stream));
            }
            Err(e) => {
                let mut client_stream = client_copy.lock().await;
                *client_stream = None;

                error!("Error accepting connection: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn handle_client(client: Arc<Mutex<Option<TcpStream>>>, stream: TcpStream) {
    let mut client_stream = client.lock().await;
    *client_stream = Some(stream);

    //loop {
    //    info!("Sending PING to client...");
    //    if let Err(e) = stream.write_all(b"PING\n").await {
    //        error!("Error writing to stream: {:?}", e);
    //        break;
    //    }
    //
    //    sleep(Duration::from_secs(60)).await;
    //}
}
