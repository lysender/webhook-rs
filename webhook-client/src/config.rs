use clap::Parser;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

use webhook::{Error, Result};

pub const RUST_LOG: &str = "RUST_LOG";

#[derive(Clone, Debug, Deserialize)]
pub struct ProxyTarget {
    pub host: String,
    pub secure: bool,
    pub source_path: String,
    pub dest_path: String,
}

#[derive(Clone, Deserialize)]
pub struct Config {
    pub server_address: String,
    pub targets: Vec<ProxyTarget>,
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

        if config.server_address.len() == 0 {
            return Err(Error::ConfigInvalidError(
                "Websocket address must not be empty.".to_string(),
            ));
        }

        // Simple validation for proxy targets
        for target in config.targets.iter() {
            if target.host.is_empty() {
                return Err(Error::ConfigInvalidError(
                    "Proxy target host is required.".to_string(),
                ));
            }
            if target.source_path.is_empty() {
                return Err(Error::ConfigInvalidError(
                    "Proxy target source path is required.".to_string(),
                ));
            }
            if !target.source_path.starts_with("/") {
                return Err(Error::ConfigInvalidError(
                    "Proxy target source path is invalid.".to_string(),
                ));
            }
            if target.dest_path.is_empty() {
                return Err(Error::ConfigInvalidError(
                    "Proxy target destination path is required.".to_string(),
                ));
            }
            if !target.dest_path.starts_with("/") {
                return Err(Error::ConfigInvalidError(
                    "Proxy target destination path is invalid.".to_string(),
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

/// webhook-client: Your app's backdoor
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct AppArgs {
    #[arg(short, long, value_name = "config.toml")]
    pub config: PathBuf,
}
