use clap::Parser;
use std::{process, sync::Arc};

pub mod client;
pub mod config;
pub mod context;

use crate::client::start_client;
use crate::config::{AppArgs, Config, RUST_LOG};
use crate::context::Context;

use webhook::Result;

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

    let args = AppArgs::parse();

    if let Err(e) = run_command(args).await {
        eprintln!("Application error: {e}");
        process::exit(1);
    }
}

async fn run_command(args: AppArgs) -> Result<()> {
    let config = Config::build(args.config.as_path())?;
    let ctx = Arc::new(Context::new(config));
    start_client(ctx).await;

    Ok(())
}
