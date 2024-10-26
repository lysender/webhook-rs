use clap::Parser;
use std::{process, sync::Arc};
use tracing::error;

use webhook_rs::config::{ServerAppArgs, ServerConfig, RUST_LOG};
use webhook_rs::context::ServerContext;
use webhook_rs::tunnel::start_tunnel_server;
use webhook_rs::web::start_web_server;
use webhook_rs::{Error, Result};

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

    let args = ServerAppArgs::parse();

    if let Err(e) = run_command(args).await {
        eprintln!("Application error: {e}");
        process::exit(1);
    }
}

async fn run_command(args: ServerAppArgs) -> Result<()> {
    let config = ServerConfig::build(args.config.as_path())?;
    let ctx = Arc::new(ServerContext::new(config));

    let res = tokio::try_join!(start_tunnel_server(ctx.clone()), start_web_server(ctx));

    if let Err(e) = res {
        let msg = format!("Error starting servers: {e}");
        error!("{}", msg);
        return Err(Error::AnyError(msg));
    }

    Ok(())
}
