use std::collections::{HashMap, VecDeque};
use tokio::{
    sync::{Mutex, Notify},
    time::{timeout, Duration},
};
use tracing::error;

use crate::{message::TunnelMessage2, Result};

pub struct MessageMap {
    messages: Mutex<HashMap<u128, TunnelMessage2>>,
    notify: Notify,
}

pub enum QueueMessage {
    Message(TunnelMessage2),
    Ping,
}

pub struct MessageQueue {
    messages: Mutex<VecDeque<QueueMessage>>,
    notify: Notify,
}

impl MessageQueue {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
        }
    }

    pub async fn push(&self, message: QueueMessage) {
        {
            let mut messages = self.messages.lock().await;
            messages.push_back(message);
        }
        self.notify.notify_waiters();
    }

    pub async fn pop(&self) -> Option<QueueMessage> {
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

    pub async fn clear(&self) {
        {
            let mut messages = self.messages.lock().await;
            messages.clear();
        }
        self.notify.notify_waiters();
    }
}

impl MessageMap {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(HashMap::new()),
            notify: Notify::new(),
        }
    }

    pub async fn add(&self, message: TunnelMessage2) {
        {
            let mut messages = self.messages.lock().await;
            let id = message.header.id.as_u128();
            messages.insert(id, message);
        }
        self.notify.notify_waiters();
    }

    pub async fn get(&self, id: &u128) -> Result<TunnelMessage2> {
        // Try to get the message within 10 seconds and give up after that
        match timeout(Duration::from_secs(10), self.get_inner(id)).await {
            Ok(res) => Ok(res.expect("Message must be present in the map.")),
            Err(_) => {
                let msg = "Message map getter timeout.";
                error!("{}", msg);
                Err(msg.into())
            }
        }
    }

    async fn get_inner(&self, id: &u128) -> Option<TunnelMessage2> {
        // Keep trying to get the message until it becomes available
        let message: Option<TunnelMessage2>;
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

    pub async fn clear(&self) {
        let mut messages = self.messages.lock().await;
        messages.clear();
    }
}
