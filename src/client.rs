use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::Result;

pub struct TunnelClient {
    stream: Option<TcpStream>,
    verified: bool,
}

impl TunnelClient {
    pub fn new() -> Self {
        TunnelClient {
            stream: None,
            verified: false,
        }
    }

    pub fn with_stream(stream: TcpStream) -> Self {
        Self {
            stream: Some(stream),
            verified: false,
        }
    }

    pub fn verify(&mut self) {
        self.verified = true;
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    pub fn is_verified(&self) -> bool {
        self.verified && self.is_connected()
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if let Some(stream) = self.stream.as_mut() {
            return match stream.read(buf).await {
                Ok(n) => Ok(n),
                Err(e) => {
                    let msg = format!("Read client stream failed: {}", e);
                    Err(msg.into())
                }
            };
        }

        // No connection yet
        return Err("Read client stream failed: no client connection yet.".into());
    }

    pub async fn write(&mut self, data: &[u8]) -> Result<()> {
        if let Some(stream) = self.stream.as_mut() {
            return match stream.write_all(data).await {
                Ok(_) => Ok(()),
                Err(write_err) => {
                    let msg = format!("Write to client stream failed: {}", write_err);
                    Err(msg.into())
                }
            };
        }

        Err("Write to client stream failed: no client connection yet.".into())
    }
}
