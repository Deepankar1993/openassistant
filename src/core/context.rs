// src/core/context.rs
//! Session context — backward compatible with new persona system

use super::persona::{FullContext, Persona, UserModel};

/// Legacy Context struct — kept for backward compatibility
/// New code should use FullContext from persona module
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Context {
    pub session_count: usize,
    pub total_messages: usize,
    pub topics: Vec<String>,
    pub preferences: serde_json::Value,
}

impl Context {
    pub fn new() -> Self {
        Self {
            session_count: 0,
            total_messages: 0,
            topics: Vec::new(),
            preferences: serde_json::json!({}),
        }
    }

    pub fn summary(&self) -> String {
        serde_json::json!({
            "sessions": self.session_count,
            "messages": self.total_messages,
            "topics": self.topics,
            "preferences": self.preferences,
        })
        .to_string()
    }

    pub fn record_topic(&mut self, topic: impl Into<String>) {
        let topic = topic.into();
        if !self.topics.contains(&topic) {
            self.topics.push(topic);
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
