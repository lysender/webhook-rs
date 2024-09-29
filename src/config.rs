use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use url::Url;

use crate::{utils::valid_webhook_path, Error, Result};

pub const RUST_LOG: &str = "RUST_LOG";

#[derive(Clone, Deserialize)]
pub struct ServerConfig {
    pub web_address: String,
    pub tunnel_port: u16,
    pub webhook_path: String,
    pub jwt_secret: String,
}

#[derive(Clone, Deserialize)]
pub struct ClientConfig {
    pub tunnel_address: String,
    pub target_url: String,
    pub jwt_secret: String,
}

impl ServerConfig {
    pub fn build(filename: &Path) -> Result<ServerConfig> {
        let toml_string = match fs::read_to_string(filename) {
            Ok(str) => str,
            Err(error) => {
                return Err(Error::ConfigReadError(error.to_string()));
            }
        };

        let config: ServerConfig = match toml::from_str(toml_string.as_str()) {
            Ok(value) => value,
            Err(error) => return Err(Error::ConfigParseError(error.to_string())),
        };

        if config.webhook_path.len() == 0 {
            return Err(Error::ConfigInvalidError(
                "Webhook path must not be empty.".to_string(),
            ));
        }

        if !valid_webhook_path(config.webhook_path.as_str()) {
            return Err(Error::ConfigInvalidError(
                "Webhook path must be in this format: /foo-bar".to_string(),
            ));
        }

        if config.jwt_secret.len() == 0 {
            return Err(Error::ConfigInvalidError(
                "JWT secret must no be empty.".to_string(),
            ));
        }

        Ok(config)
    }
}

impl ClientConfig {
    pub fn build(filename: &Path) -> Result<ClientConfig> {
        let toml_string = match fs::read_to_string(filename) {
            Ok(str) => str,
            Err(error) => {
                return Err(Error::ConfigReadError(error.to_string()));
            }
        };

        let config: ClientConfig = match toml::from_str(toml_string.as_str()) {
            Ok(value) => value,
            Err(error) => return Err(Error::ConfigParseError(error.to_string())),
        };

        if config.tunnel_address.len() == 0 {
            return Err(Error::ConfigInvalidError(
                "Tunnel address must not be empty.".to_string(),
            ));
        }

        if config.target_url.len() == 0 {
            return Err(Error::ConfigInvalidError(
                "Target application URL must not be empty.".to_string(),
            ));
        }

        if let Err(_) = Url::parse(config.target_url.as_str()) {
            return Err(Error::ConfigInvalidError(
                "Target application URL must be a valid URL.".to_string(),
            ));
        }

        if config.jwt_secret.len() == 0 {
            return Err(Error::ConfigInvalidError(
                "JWT secret must no be empty.".to_string(),
            ));
        }

        Ok(config)
    }
}

/// webhook-rs: A webhook server and client
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct AppArgs {
    #[command(subcommand)]
    pub command: Commands,

    /// TOML configuration file
    #[arg(short, long, value_name = "FILE.toml")]
    pub config: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Runs the Webhook server
    Server,

    /// Runs the Webhook client
    Client,
}
