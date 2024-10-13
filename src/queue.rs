use std::collections::VecDeque;

use tokio::sync::{Mutex, Notify};

use crate::message::TunnelMessage;

pub struct MessageQueue {
    messages: Mutex<VecDeque<TunnelMessage>>,
    notify: Notify,
}

impl MessageQueue {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
        }
    }
}
