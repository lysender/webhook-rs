use std::sync::Arc;

use crate::{
    config::{ClientConfig, ServerConfig},
    message::TunnelMessage,
    queue::{MessageMap, MessageQueue},
    tunnel::TunnelState,
    Result,
};

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
    tunnel_state: TunnelState,
    req_queue: MessageQueue,
    config: ClientConfig,
}
