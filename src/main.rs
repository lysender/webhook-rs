use clap::Parser;
use client::start_client;
use queue::{MessageMap, MessageQueue};
use std::{process, sync::Arc};

mod client;
mod config;
mod error;
mod message;
mod queue;
mod token;
mod tunnel;
mod utils;
mod web;

use config::{AppArgs, ClientConfig, Commands, ServerConfig, RUST_LOG};

// Re-exports
pub use error::{Error, Result};

use tracing::error;
use tunnel::start_tunnel_server;
use web::start_web_server;

#[tokio::main]
async fn main() {
    // Set the RUST_LOG, if it hasn't been explicitly defined
    if std::env::var(RUST_LOG).is_err() {
        std::env::set_var(RUST_LOG, "webhook_rs=info")
    }

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let args = AppArgs::parse();

    if let Err(e) = run_command(args).await {
        eprintln!("Application error: {e}");
        process::exit(1);
    }
}

async fn run_command(args: AppArgs) -> Result<()> {
    match args.command {
        Commands::Server => run_server(&args).await,
        Commands::Client => run_client(&args).await,
    }
}

async fn run_server(args: &AppArgs) -> Result<()> {
    let config = ServerConfig::build(args.config.as_path())?;
    let arc_config = Arc::new(config);

    let req_queue = Arc::new(MessageQueue::new());
    let res_map = Arc::new(MessageMap::new());

    let res = tokio::try_join!(
        start_tunnel_server(arc_config.clone(), req_queue.clone(), res_map.clone()),
        start_web_server(arc_config, req_queue, res_map)
    );

    if let Err(e) = res {
        let msg = format!("Error starting servers: {e}");
        error!("{}", msg);
        return Err(Error::AnyError(msg));
    }

    Ok(())
}

async fn run_client(args: &AppArgs) -> Result<()> {
    let config = ClientConfig::build(args.config.as_path())?;
    start_client(&config).await;

    Ok(())
}
