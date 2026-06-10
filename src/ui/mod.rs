// src/ui/mod.rs
//! Interactive UI system for openAssistant
//! Provides both TUI (terminal) and Web UI interfaces

pub mod tui;

use crate::core::Message;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared application state between UI and agent
pub struct AppState {
    pub messages: Vec<Message>,
    pub input_buffer: String,
    pub is_processing: bool,
    pub status_message: String,
    pub model_name: String,
    pub workspace_dir: String,
    pub permission_mode: String,
    /// Token usage tracking
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost: f64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input_buffer: String::new(),
            is_processing: false,
            status_message: "Ready. Type a message or /help for commands.".to_string(),
            model_name: "openrouter/owl-alpha".to_string(),
            workspace_dir: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            permission_mode: "Default".to_string(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cost: 0.0,
        }
    }
}

impl AppState {
    pub fn add_message(&mut self, role: &str, content: &str) {
        match role {
            "user" => self.messages.push(Message::user(content)),
            "assistant" => self.messages.push(Message::assistant(content)),
            "system" => self.messages.push(Message::system(content)),
            "tool" => self.messages.push(Message::tool(content)),
            _ => self.messages.push(Message::assistant(content)),
        }
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.status_message = "Conversation cleared.".to_string();
    }

    pub fn update_status(&mut self, msg: &str) {
        self.status_message = msg.to_string();
    }
}
