use tokio::io::AsyncWriteExt;
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, Duration},
};
use tracing::{error, info};

use crate::{config::Config, Result};

pub async fn start_relay_server(config: &Config) -> Result<()> {
    info!("Starting relay server...");
    let address = format!("0.0.0.0:{}", config.tunnel_port);
    let listener = TcpListener::bind(address.as_str()).await.unwrap();

    info!("Webhook relay server started at {}", address);

    loop {
        let res = listener.accept().await;
        match res {
            Ok((stream, addr)) => {
                info!("Connection established: {:?}", addr);
                tokio::spawn(handle_client(stream));
            }
            Err(e) => {
                error!("Error accepting connection: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn handle_client(mut stream: TcpStream) {
    loop {
        info!("Sending PING to client...");
        if let Err(e) = stream.write_all(b"PING\n").await {
            error!("Error writing to stream: {:?}", e);
            break;
        }

        sleep(Duration::from_secs(60)).await;
    }
}
