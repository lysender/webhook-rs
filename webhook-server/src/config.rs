use clap::Parser;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

use webhook::utils::valid_webhook_path;
use webhook::{Error, Result};

pub const RUST_LOG: &str = "RUST_LOG";

#[derive(Clone, Deserialize)]
pub struct Config {
    pub web_address: String,
    pub webhook_path: String,
    pub jwt_secret: String,
}

impl Config {
    pub fn build(filename: &Path) -> Result<Config> {
        let toml_string = match fs::read_to_string(filename) {
            Ok(str) => str,
            Err(error) => {
                return Err(Error::ConfigReadError(error.to_string()));
            }
        };

        let config: Config = match toml::from_str(toml_string.as_str()) {
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

/// webhook-server: Your app's backdoor
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct AppArgs {
    #[arg(short, long, value_name = "config.toml")]
    pub config: PathBuf,
}
