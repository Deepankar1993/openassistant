// src/core/channels.rs
//! Channels — push external events (webhooks, cron, file watches) into running sessions
//! Like Claude Code's --brief mode and SendUserMessage tool

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, broadcast};
use tracing::{info, debug, warn};

// ─── Channel Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChannelType {
    /// Webhook endpoint
    Webhook { path: String },
    /// Cron schedule (interval in seconds)
    Cron { interval_seconds: u64 },
    /// File system watch
    FileWatch { path: String },
    /// Stdin input
    Stdin,
}

// ─── Channel Message ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub channel: String,
    pub session_id: String,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<HashMap<String, String>>,
}

// ─── Channel Listener ─────────────────────────────────────────────────

#[derive(Debug)]
pub struct ChannelListener {
    pub name: String,
    pub channel_type: ChannelType,
    pub session_id: String,
    pub tx: mpsc::Sender<ChannelMessage>,
    pub enabled: bool,
}

// ─── Channel Manager ──────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ChannelManager {
    listeners: HashMap<String, ChannelListener>,
    /// Broadcast channel for sending messages to all sessions
    broadcast_tx: Option<broadcast::Sender<ChannelMessage>>,
}

impl ChannelManager {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Self {
            listeners: HashMap::new(),
            broadcast_tx: Some(tx),
        }
    }

    /// Register a new channel listener
    pub fn register(
        &mut self,
        name: &str,
        session_id: &str,
        channel_type: ChannelType,
    ) -> mpsc::Receiver<ChannelMessage> {
        let (tx, rx) = mpsc::channel(128);
        let listener = ChannelListener {
            name: name.to_string(),
            channel_type,
            session_id: session_id.to_string(),
            tx,
            enabled: true,
        };
        info!("Registered channel '{}' for session {}", name, session_id);
        self.listeners.insert(name.to_string(), listener);
        rx
    }

    /// Push a message to a specific session
    pub fn push_to_session(&self, session_id: &str, content: &str) -> Result<()> {
        let msg = ChannelMessage {
            channel: "direct".to_string(),
            session_id: session_id.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            metadata: None,
        };

        // Send to broadcast channel
        if let Some(ref tx) = self.broadcast_tx {
            let _ = tx.send(msg);
        }

        // Also send to specific listener if exists
        for listener in self.listeners.values() {
            if listener.session_id == session_id && listener.enabled {
                let _ = listener.tx.try_send(ChannelMessage {
                    channel: "direct".to_string(),
                    session_id: session_id.to_string(),
                    content: content.to_string(),
                    timestamp: chrono::Utc::now(),
                    metadata: None,
                });
            }
        }

        Ok(())
    }

    /// Push a message to all sessions (broadcast)
    pub fn broadcast(&self, content: &str) -> Result<()> {
        let msg = ChannelMessage {
            channel: "broadcast".to_string(),
            session_id: "all".to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            metadata: None,
        };

        if let Some(ref tx) = self.broadcast_tx {
            let _ = tx.send(msg);
        }

        info!("Broadcast message to all sessions: {}", &content[..content.len().min(80)]);
        Ok(())
    }

    /// Subscribe to the broadcast channel
    pub fn subscribe(&self) -> Option<broadcast::Receiver<ChannelMessage>> {
        self.broadcast_tx.as_ref().map(|tx| tx.subscribe())
    }

    pub fn list_listeners(&self) -> Vec<&ChannelListener> {
        self.listeners.values().collect()
    }

    pub fn enable_listener(&mut self, name: &str, enabled: bool) -> bool {
        if let Some(listener) = self.listeners.get_mut(name) {
            listener.enabled = enabled;
            true
        } else {
            false
        }
    }
}

// ─── SendUserMessage Tool (for --brief mode) ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendUserMessage {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl SendUserMessage {
    pub fn execute(&self, manager: &ChannelManager) -> Result<()> {
        let session = self.session_id.as_deref().unwrap_or("default");
        manager.push_to_session(session, &self.content)?;
        info!("SendUserMessage sent to session '{}'", session);
        Ok(())
    }
}
