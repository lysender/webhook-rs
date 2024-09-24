use dotenvy::dotenv;
use serde::Deserialize;
use std::env;

use crate::Result;

pub const WEB_PORT: &str = "WEB_PORT";
pub const TUNNEL_PORT: &str = "TUNNEL_PORT";

#[derive(Clone, Deserialize)]
pub struct Config {
    pub web_port: u16,
    pub tunnel_port: u16,
}

impl Config {
    pub fn build() -> Result<Config> {
        dotenv().ok();

        let web_port_str = env::var(WEB_PORT).expect("WEB_PORT is not set");
        let web_port: u16 = web_port_str
            .parse()
            .expect("WEB_PORT is not a valid number");

        let tunnel_port_str = env::var(TUNNEL_PORT).expect("TUNNEL_PORT is not set");
        let tunnel_port: u16 = tunnel_port_str
            .parse()
            .expect("TUNNEL_PORT is not a valid number");

        Ok(Config {
            web_port,
            tunnel_port,
        })
    }
}
