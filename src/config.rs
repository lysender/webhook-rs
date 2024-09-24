use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use serde::Deserialize;
use std::env;

use crate::Result;

pub const WEB_PORT: &str = "WEB_PORT";
pub const TUNNEL_PORT: &str = "TUNNEL_PORT";
pub const SERVER_ADDRESS: &str = "SERVER_ADDRESS";
pub const TARGET_URL: &str = "TARGET_URL";

#[derive(Clone, Deserialize)]
pub struct ServerConfig {
    pub web_port: u16,
    pub tunnel_port: u16,
}

#[derive(Clone, Deserialize)]
pub struct ClientConfig {
    pub server_address: String,
    pub target_url: String,
}

impl ServerConfig {
    pub fn build() -> Result<ServerConfig> {
        dotenv().ok();

        let web_port_str = env::var(WEB_PORT).expect("WEB_PORT is not set");
        let web_port: u16 = web_port_str
            .parse()
            .expect("WEB_PORT is not a valid number");

        let tunnel_port_str = env::var(TUNNEL_PORT).expect("TUNNEL_PORT is not set");
        let tunnel_port: u16 = tunnel_port_str
            .parse()
            .expect("TUNNEL_PORT is not a valid number");

        Ok(ServerConfig {
            web_port,
            tunnel_port,
        })
    }
}

impl ClientConfig {
    pub fn build() -> Result<ClientConfig> {
        dotenv().ok();

        let server_address: String = env::var(SERVER_ADDRESS).expect("SERVER_ADDRESS is not set");
        let target_url: String = env::var(TARGET_URL).expect("TARGET_URL is not set");

        Ok(ClientConfig {
            server_address,
            target_url,
        })
    }
}

/// webhook-rs Webhook client/server application
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Runs the web and relay server
    Server,

    /// Runs the client
    Client,
}
