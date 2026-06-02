// src/security/pairing.rs
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

/// DM pairing: unknown senders receive a pairing code
#[derive(Debug)]
pub struct PairManager {
    pending: Arc<Mutex<HashMap<String, PairingRequest>>>,
}

#[derive(Debug, Clone)]
pub struct PairingRequest {
    pub code: String,
    pub user_id: String,
    pub channel: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl PairManager {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create_request(&self, user_id: &str, channel: &str) -> String {
        let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
        let request = PairingRequest {
            code: code.clone(),
            user_id: user_id.to_string(),
            channel: channel.to_string(),
            created_at: chrono::Utc::now(),
        };
        let mut pending = self.pending.lock().await;
        pending.insert(code.clone(), request);
        info!("Created pairing request for {} on {}: {}", user_id, channel, code);
        code
    }

    pub async fn approve(&self, code: &str) -> Option<PairingRequest> {
        let mut pending = self.pending.lock().await;
        let request = pending.remove(code);
        if request.is_some() {
            info!("Approved pairing code: {}", code);
        }
        request
    }
}

impl Default for PairManager {
    fn default() -> Self {
        Self::new()
    }
}
