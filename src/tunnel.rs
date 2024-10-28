use tokio::sync::Mutex;

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
