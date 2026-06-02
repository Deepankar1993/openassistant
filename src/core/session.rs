// src/core/session.rs
use super::Message;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub channel: String,
    pub user_id: String,
    pub messages: Vec<Message>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

impl Session {
    pub fn new(channel: impl Into<String>, user_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            channel: channel.into(),
            user_id: user_id.into(),
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }

    pub fn add_message(&mut self, msg: Message) {
        self.messages.push(msg);
        self.updated_at = Utc::now();
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn last_n_messages(&self, n: usize) -> &[Message] {
        let start = self.messages.len().saturating_sub(n);
        &self.messages[start..]
    }

    pub fn summary(&self) -> String {
        format!(
            "Session {} on {} with {} messages",
            self.id,
            self.channel,
            self.messages.len()
        )
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new("cli", "local")
    }
}
