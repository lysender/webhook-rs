use std::collections::{HashMap, VecDeque};
use tokio::{
    sync::{Mutex, Notify},
    time::{timeout, Duration},
};
use tracing::error;

use crate::{message::TunnelMessage, Result};

pub struct MessageMap {
    messages: Mutex<HashMap<u128, TunnelMessage>>,
    notify: Notify,
}

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
        self.notify.notify_waiters();
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

impl MessageMap {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(HashMap::new()),
            notify: Notify::new(),
        }
    }

    pub async fn add(&self, message: TunnelMessage) {
        {
            let mut messages = self.messages.lock().await;
            let id = message.id.as_u128();
            messages.insert(id, message);
        }
        self.notify.notify_waiters();
    }

    pub async fn get(&self, id: &u128) -> Result<TunnelMessage> {
        // Try to get the message within 15 seconds and give up after that
        match timeout(Duration::from_secs(15), self.get_inner(id)).await {
            Ok(res) => Ok(res.expect("Message must be present in the map.")),
            Err(_) => {
                let msg = "Message map getter timeout.";
                error!("{}", msg);
                Err(msg.into())
            }
        }
    }

    async fn get_inner(&self, id: &u128) -> Option<TunnelMessage> {
        // Keep trying to get the message until it becomes available
        let message: Option<TunnelMessage>;
        loop {
            let maybe_message = {
                let mut messages = self.messages.lock().await;
                messages.remove(id)
            };

            if let Some(m) = maybe_message {
                message = Some(m);
                break;
            } else {
                self.notify.notified().await;
            }
        }
        message
    }
}
