use std::env;

use webhook::utils::valid_webhook_path;
use webhook::{Error, Result};

pub const RUST_LOG: &str = "RUST_LOG";
pub const WEBHOOK_SERVER_WEB_ADDRESS: &str = "WEBHOOK_SERVER_WEB_ADDRESS";
pub const WEBHOOK_SERVER_WEBHOOK_PATH: &str = "WEBHOOK_SERVER_WEBHOOK_PATH";
pub const WEBHOOK_SERVER_JWT_SECRET: &str = "WEBHOOK_SERVER_JWT_SECRET";

#[derive(Clone)]
pub struct Config {
    pub web_address: String,
    pub webhook_path: String,
    pub jwt_secret: String,
}

impl Config {
    pub fn build() -> Result<Config> {
        let web_address = read_env(WEBHOOK_SERVER_WEB_ADDRESS)?;
        let webhook_path = read_env(WEBHOOK_SERVER_WEBHOOK_PATH)?;
        let jwt_secret = read_env(WEBHOOK_SERVER_JWT_SECRET)?;

        let config = Config {
            web_address,
            webhook_path,
            jwt_secret,
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

fn read_env(key: &str) -> Result<String> {
    match env::var(key) {
        Ok(value) => {
            if value.trim().is_empty() {
                Err(Error::ConfigInvalidError(format!(
                    "Environment variable {key} must not be empty."
                )))
            } else {
                Ok(value)
            }
        }
        Err(_) => Err(Error::ConfigInvalidError(format!(
            "Missing environment variable: {key}."
        ))),
    }
}
