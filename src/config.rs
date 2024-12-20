use clap::Parser;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{utils::valid_webhook_path, Error, Result};

pub const RUST_LOG: &str = "RUST_LOG";

#[derive(Clone, Deserialize)]
pub struct ServerConfig {
    pub web_address: String,
    pub webhook_path: String,
    pub jwt_secret: String,
}

#[derive(Clone, Deserialize)]
pub struct ClientConfig {
    pub ws_address: String,
    pub target_address: String,
    pub target_secure: bool,
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

        // Allow either "*" or "/foo-bar" format
        let wh_path = config.webhook_path.as_str();
        if wh_path != "*" {
            if !valid_webhook_path(wh_path) {
                return Err(Error::ConfigInvalidError(
                    "Webhook path must be in this format: /foo-bar".to_string(),
                ));
            }
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

        if config.ws_address.len() == 0 {
            return Err(Error::ConfigInvalidError(
                "Websocket address must not be empty.".to_string(),
            ));
        }

        if config.target_address.len() == 0 {
            return Err(Error::ConfigInvalidError(
                "Target application host must not be empty.".to_string(),
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

/// webhook-server: Your app's backdoor
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct ServerAppArgs {
    #[arg(short, long, value_name = "config.toml")]
    pub config: PathBuf,
}

/// webhook-client: Your app's backdoor
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct ClientAppArgs {
    #[arg(short, long, value_name = "config.toml")]
    pub config: PathBuf,
}
