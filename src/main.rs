use clap::Parser;
use std::process;

mod config;
mod error;
mod relay;
mod web;
mod workers;

use config::{Args, Commands, ServerConfig};

// Re-exports
pub use error::{Error, Result};

use web::start_web_server;

fn main() {
    // Set the RUST_LOG, if it hasn't been explicitly defined
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "webhook_rs=info")
    }

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let args = Args::parse();

    if let Err(e) = run_command(args) {
        eprintln!("Application error: {e}");
        process::exit(1);
    }
}

fn run_command(arg: Args) -> Result<()> {
    match arg.command {
        Commands::Server => {
            let config = ServerConfig::build()?;
            start_web_server(&config);
            Ok(())
        }
        Commands::Client => Ok(()),
    }
}
