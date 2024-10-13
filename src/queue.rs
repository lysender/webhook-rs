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

    pub async fn push(&self, message: TunnelMessage) {
        {
            let mut messages = self.messages.lock().await;
            messages.push_back(message);
        }
        self.notify.notify_one();
    }

    pub async fn pop(&self) -> Option<TunnelMessage> {
        let maybe_message = {
            let mut messages = self.messages.lock().await;
            messages.pop_front()
        };

        match maybe_message {
            Some(message) => Some(message),
            None => {
                self.notify.notified().await;
                None
            }
        }
    }
}
