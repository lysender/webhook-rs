use clap::Parser;
use client::start_client;
use context::{ClientContext, ServerContext};
use std::{path::PathBuf, process, sync::Arc};

mod client;
mod config;
mod context;
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
        Commands::Server { config } => run_server(config).await,
        Commands::Client { config } => run_client(config).await,
        Commands::Genkey => {
            println!("generate some encryption key here...");
            Ok(())
        }
    }
}

async fn run_server(config_path: PathBuf) -> Result<()> {
    let config = ServerConfig::build(config_path.as_path())?;
    let ctx = Arc::new(ServerContext::new(config));

    let res = tokio::try_join!(start_tunnel_server(ctx.clone()), start_web_server(ctx));

    if let Err(e) = res {
        let msg = format!("Error starting servers: {e}");
        error!("{}", msg);
        return Err(Error::AnyError(msg));
    }

    Ok(())
}

async fn run_client(config_path: PathBuf) -> Result<()> {
    let config = ClientConfig::build(config_path.as_path())?;
    let ctx = Arc::new(ClientContext::new(config));
    start_client(ctx).await;

    Ok(())
}
