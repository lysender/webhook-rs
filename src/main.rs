use std::{process, sync::Arc};

mod config;
mod error;
mod relay;
mod web;

use config::Config;

// Re-exports
pub use error::{Error, Result};

use relay::start_relay_server;
use tokio::{net::TcpStream, sync::Mutex};
use tracing::error;
use web::start_web_server;

#[tokio::main]
async fn main() {
    // Set the RUST_LOG, if it hasn't been explicitly defined
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "webhook_rs=info")
    }

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    if let Err(e) = run().await {
        eprintln!("Application error: {e}");
        process::exit(1);
    }
}

async fn run() -> Result<()> {
    let config = Config::build()?;
    let client: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));

    let res = tokio::try_join!(
        start_relay_server(client.clone(), &config),
        start_web_server(client.clone(), &config)
    );
    if let Err(e) = res {
        let msg = format!("Error starting servers: {e}");
        error!("{}", msg);
        return Err(Error::AnyError(msg));
    }

    Ok(())
}
