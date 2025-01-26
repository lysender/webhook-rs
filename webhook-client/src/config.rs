use clap::Parser;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

use zerror::{Error, Result};

pub const RUST_LOG: &str = "RUST_LOG";

#[derive(Clone, Deserialize)]
pub struct Config {
    pub ws_address: String,
    pub target_address: String,
    pub target_secure: bool,
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

/// webhook-client: Your app's backdoor
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct AppArgs {
    #[arg(short, long, value_name = "config.toml")]
    pub config: PathBuf,
}
