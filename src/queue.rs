use std::{collections::VecDeque, sync::Arc};

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
            println!("Messages after push: {}", messages.len());
        }
        self.notify.notify_waiters();
    }

    pub async fn pop(&self) -> Option<TunnelMessage> {
        let maybe_message = {
            let mut messages = self.messages.lock().await;
            let msg = messages.pop_front();

            println!("Messages after pop: {}", messages.len());
            msg
        };

        match maybe_message {
            Some(message) => Some(message),
            None => {
                self.notify.notified().await;
                println!("Notified, trying to pop again.");
                None
            }
        }
    }
}
