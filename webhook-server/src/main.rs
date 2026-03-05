use std::{process, sync::Arc};
use tracing::error;

use crate::config::{Config, RUST_LOG};
use crate::context::Context;
use crate::web::start_web_server;
use webhook::{Error, Result};

pub mod config;
pub mod context;
pub mod web;

#[tokio::main]
async fn main() {
    // Set the RUST_LOG, if it hasn't been explicitly defined
    if std::env::var(RUST_LOG).is_err() {
        std::env::set_var(RUST_LOG, "webhook_server=info")
    }

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    if let Err(e) = run_command().await {
        eprintln!("Application error: {e}");
        process::exit(1);
    }
}

async fn run_command() -> Result<()> {
    let config = Config::build()?;
    let ctx = Arc::new(Context::new(config));

    if let Err(e) = start_web_server(ctx).await {
        let msg = format!("Error starting servers: {e}");
        error!("{}", msg);
        return Err(Error::AnyError(msg));
    }

    Ok(())
}
