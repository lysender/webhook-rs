use clap::Parser;
use std::{process, sync::Arc};

use webhook_rs::client::start_client;
use webhook_rs::config::{ClientAppArgs, ClientConfig, RUST_LOG};
use webhook_rs::context::ClientContext;

use webhook_rs::Result;

#[tokio::main]
async fn main() {
    // Set the RUST_LOG, if it hasn't been explicitly defined
    if std::env::var(RUST_LOG).is_err() {
        std::env::set_var(RUST_LOG, "webhook_client=info")
    }

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let args = ClientAppArgs::parse();

    if let Err(e) = run_command(args).await {
        eprintln!("Application error: {e}");
        process::exit(1);
    }
}

async fn run_command(args: ClientAppArgs) -> Result<()> {
    let config = ClientConfig::build(args.config.as_path())?;
    let ctx = Arc::new(ClientContext::new(config));
    start_client(ctx).await;

    Ok(())
}
