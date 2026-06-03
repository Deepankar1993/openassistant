// src/core/mod.rs
pub mod agent;
pub mod claude_bridge;
pub mod session;
pub mod context;
pub mod persona;
pub mod memory;
pub mod self_update;
pub mod browser;
pub mod hooks;
pub mod subagent;
pub mod standing_orders;
pub mod multi_agent;
pub mod web_search;
pub mod goal_system;
pub mod goal_store;
pub mod permissions;
pub mod checkpoint;
pub mod worktree;
pub mod agent_teams;
pub mod mcp;
pub mod streaming;
pub mod plugins;
pub mod workflows;
pub mod channels;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<serde_json::Value>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: "user".to_string(),
            content: content.into(),
            timestamp: chrono::Utc::now(),
            metadata: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: "assistant".to_string(),
            content: content.into(),
            timestamp: chrono::Utc::now(),
            metadata: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: "system".to_string(),
            content: content.into(),
            timestamp: chrono::Utc::now(),
            metadata: None,
        }
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: "tool".to_string(),
            content: content.into(),
            timestamp: chrono::Utc::now(),
            metadata: None,
        }
    }
}
