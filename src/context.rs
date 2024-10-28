use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    config::{ClientConfig, ServerConfig},
    message::TunnelMessage,
    queue::{MessageMap, MessageQueue},
    Result,
};

pub struct TunnelState {
    verified: Mutex<bool>,
}

impl TunnelState {
    pub fn new() -> Self {
        Self {
            verified: Mutex::new(false),
        }
    }

    pub async fn is_verified(&self) -> bool {
        let verified = self.verified.lock().await;
        *verified
    }

    pub async fn verify(&self) {
        let mut verified = self.verified.lock().await;
        *verified = true;
    }

    pub async fn reset(&self) {
        let mut verified = self.verified.lock().await;
        *verified = false;
    }
}

pub struct ServerContext {
    pub config: Arc<ServerConfig>,

    tunnel_state: TunnelState,
    req_queue: MessageQueue,
    res_map: MessageMap,
}

impl ServerContext {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            tunnel_state: TunnelState::new(),
            req_queue: MessageQueue::new(),
            res_map: MessageMap::new(),
            config: Arc::new(config),
        }
    }

    pub async fn is_verified(&self) -> bool {
        self.tunnel_state.is_verified().await
    }

    pub async fn verify(&self) {
        self.tunnel_state.verify().await;
    }

    pub async fn unverify(&self) {
        self.tunnel_state.reset().await;
    }

    pub async fn add_request(&self, message: TunnelMessage) {
        self.req_queue.push(message).await;
    }

    pub async fn get_request(&self) -> Option<TunnelMessage> {
        self.req_queue.pop().await
    }

    pub async fn clear_requests(&self) {
        self.req_queue.clear().await;
    }

    pub async fn add_response(&self, message: TunnelMessage) {
        self.res_map.add(message).await;
    }

    pub async fn get_response(&self, id: &u128) -> Result<TunnelMessage> {
        self.res_map.get(id).await
    }

    pub async fn clear_responses(&self) {
        self.res_map.clear().await;
    }

    pub async fn reset(&self) {
        self.tunnel_state.reset().await;
        self.req_queue.clear().await;
        self.res_map.clear().await;
    }
}

pub struct ClientContext {
    pub config: Arc<ClientConfig>,
    tunnel_state: TunnelState,
    req_queue: MessageQueue,
}

impl ClientContext {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            tunnel_state: TunnelState::new(),
            req_queue: MessageQueue::new(),
            config: Arc::new(config),
        }
    }

    pub async fn is_verified(&self) -> bool {
        self.tunnel_state.is_verified().await
    }

    pub async fn verify(&self) {
        self.tunnel_state.verify().await;
    }

    pub async fn unverify(&self) {
        self.tunnel_state.reset().await;
    }

    pub async fn add_request(&self, message: TunnelMessage) {
        self.req_queue.push(message).await;
    }

    pub async fn get_request(&self) -> Option<TunnelMessage> {
        self.req_queue.pop().await
    }

    pub async fn clear_requests(&self) {
        self.req_queue.clear().await;
    }

    pub async fn reset(&self) {
        self.tunnel_state.reset().await;
        self.req_queue.clear().await;
    }
}
