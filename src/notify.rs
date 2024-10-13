use std::{collections::VecDeque, sync::Arc};

use tokio::{
    sync::{Mutex, Notify},
    time::{sleep, Duration},
};

#[derive(Debug, Clone, Copy)]
struct Message {
    id: u32,
}

struct SharedState {
    vec: Mutex<VecDeque<Message>>,
    notify: Notify,
}

impl SharedState {
    fn new() -> Self {
        Self {
            vec: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
        }
    }

    async fn push(&self, value: Message) {
        {
            let mut vec = self.vec.lock().await;
            vec.push_back(value);
            println!("Pushed value into vec: {}", value.id);
        }
        self.notify.notify_one();
    }

    async fn pop(&self) -> Option<Message> {
        let maybe_value = {
            let mut vec = self.vec.lock().await;
            vec.pop_front()
        };

        if let Some(value) = maybe_value {
            println!("Popped value from vec: {}", value.id);
        }

        maybe_value
    }
}

async fn push_values(state: Arc<SharedState>, values: Vec<Message>) {
    for value in values {
        state.push(value).await;
        //{
        //    let mut vec = state.vec.lock().await;
        //    vec.push_back(value);
        //    println!("Pushed value into vec: {}", value.id);
        //}
        //state.notify.notify_one();
        sleep(Duration::from_millis(500)).await;
    }
}

async fn pop_values(state: Arc<SharedState>) {
    loop {
        let _ = state.pop().await;
        sleep(Duration::from_secs(1)).await;

        //let maybe_value = {
        //    let mut vec = state.vec.lock().await;
        //    vec.pop_front()
        //};
        //
        //if let Some(value) = maybe_value {
        //    println!("Popped value from vec: {}", value.id);
        //    sleep(Duration::from_secs(1)).await;
        //} else {
        //    state.notify.notified().await;
        //}
    }
}

pub async fn test_notify() {
    let state = Arc::new(SharedState::new());

    let state_clone = state.clone();

    let push_task = tokio::spawn(async move {
        let values = vec![
            Message { id: 1 },
            Message { id: 2 },
            Message { id: 3 },
            Message { id: 4 },
            Message { id: 5 },
        ];
        push_values(state_clone, values).await;
    });

    let pop_task = tokio::spawn(async move {
        pop_values(state).await;
    });

    let _ = tokio::join!(push_task, pop_task);
}
