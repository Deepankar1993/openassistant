// src/core/persona.rs
//! Persona system — the "SOUL.md" of openAssistant
//! Combines OpenClaw's SOUL.md personality guide with Hermes's Honcho user modeling

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, debug};

/// The agent's persona — its personality, name, and behavioral guidelines.
/// Combines OpenClaw's SOUL.md concept with Hermes's evolving user model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub name: String,                   // "Lobster", "Assistant", user-chosen
    pub version: String,                // Persona format version
    pub language: String,               // Preferred language
    pub tone: String,                   // "professional", "casual", "friendly", "technical"
    pub emoji: String,                  // Signature emoji (🦞 for lobster, etc.)
    pub personality: String,            // Free-form personality description
    pub principles: Vec<String>,        // Core behavioral principles
    pub boundaries: Vec<String>,        // What the agent will NOT do
    pub capabilities: Vec<String>,      // What the agent CAN do
    pub preferences: serde_json::Value, // Agent preferences (verbosity, formatting, etc.)
}

impl Default for Persona {
    fn default() -> Self {
        Self {
            name: "openAssistant".to_string(),
            version: "1.0".to_string(),
            language: "English".to_string(),
            tone: "friendly".to_string(),
            emoji: "🦞".to_string(),
            personality: "You are a helpful, honest, and harmless AI assistant. You are curious, proactive, and genuinely care about helping the user. You have access to terminal, files, and the ability to improve yourself over time.".to_string(),
            principles: vec![
                "Be genuinely helpful, not performatively helpful".to_string(),
                "Always be honest — never make things up".to_string(),
                "Ask when you're unsure rather than guessing".to_string(),
                "Learn from every interaction".to_string(),
                "Respect user privacy and permissions".to_string(),
                "Be resourceful — try to figure things out before asking".to_string(),
            ],
            boundaries: vec![
                "Will not share private data with third parties".to_string(),
                "Will not execute destructive commands without confirmation".to_string(),
                "Will not pretend to be human".to_string(),
                "Will not access systems beyond the user's own devices".to_string(),
            ],
            capabilities: vec![
                "Terminal access (with user permission)".to_string(),
                "File reading and writing".to_string(),
                "Web search and browsing".to_string(),
                "Image analysis via Gemini CLI".to_string(),
                "Self-improvement and skill creation".to_string(),
                "Scheduled automation".to_string(),
                "Memory search and management".to_string(),
            ],
            preferences: serde_json::json!({
                "verbosity": "balanced",
                "code_style": "idiomatic",
                "response_format": "markdown",
                "emoji_usage": "moderate",
            }),
        }
    }
}

/// The user's profile — built over time through conversation (Hermes Honcho-style)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserModel {
    pub id: String,
    pub name: String,
    pub what_to_call_them: String,
    pub timezone: String,
    pub language: String,
    pub technical_level: String,       // "beginner", "intermediate", "advanced", "expert"
    pub interests: Vec<String>,
    pub projects: Vec<String>,
    pub communication_style: String,    // "concise", "detailed", "technical", "casual"
    pub preferences: serde_json::Value,
    pub notes: Vec<String>,             // Freeform notes accumulated over time
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Default for UserModel {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "User".to_string(),
            what_to_call_them: "friend".to_string(),
            timezone: "UTC".to_string(),
            language: "English".to_string(),
            technical_level: "intermediate".to_string(),
            interests: Vec::new(),
            projects: Vec::new(),
            communication_style: "balanced".to_string(),
            preferences: serde_json::json!({}),
            notes: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}

/// Combined context injected into every system prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullContext {
    pub persona: Persona,
    pub user: UserModel,
    pub session_count: usize,
    pub total_messages: usize,
    pub topics: Vec<String>,
}

impl FullContext {
    pub fn new() -> Self {
        Self {
            persona: Persona::default(),
            user: UserModel::default(),
            session_count: 0,
            total_messages: 0,
            topics: Vec::new(),
        }
    }

    /// Build the system prompt from persona + user model
    pub fn build_system_prompt(&self) -> String {
        let mut prompt = String::new();

        // Persona section
        prompt.push_str(&format!("# Identity\n"));
        prompt.push_str(&format!("You are {}, {} {}. {}\n\n",
            self.persona.emoji, self.persona.name, self.persona.version, self.persona.personality));

        // Principles
        if !self.persona.principles.is_empty() {
            push_section(&mut prompt, "Core Principles", &self.persona.principles);
        }

        // User info
        prompt.push_str("# User\n");
        prompt.push_str(&format!("Name: {}\n", self.user.name));
        prompt.push_str(&format!("What to call them: {}\n", self.user.what_to_call_them));
        prompt.push_str(&format!("Technical level: {}\n", self.user.technical_level));
        if !self.user.interests.is_empty() {
            prompt.push_str(&format!("Interests: {}\n", self.user.interests.join(", ")));
        }
        if !self.user.projects.is_empty() {
            prompt.push_str(&format!("Active projects: {}\n", self.user.projects.join(", ")));
        }
        if !self.user.notes.is_empty() {
            push_section(&mut prompt, "Notes about user", &self.user.notes);
        }
        prompt.push('\n');

        // Preferences
        push_json_section(&mut prompt, "User preferences", &self.user.preferences);
        push_json_section(&mut prompt, "Agent preferences", &self.persona.preferences);

        // Session context
        prompt.push_str(&format!("# Session\n"));
        prompt.push_str(&format!("Session count: {}\n", self.session_count));
        prompt.push_str(&format!("Total messages: {}\n", self.total_messages));
        if !self.topics.is_empty() {
            prompt.push_str(&format!("Topics discussed: {}\n", self.topics.join(", ")));
        }

        prompt
    }

    /// Update user model from conversation observation
    pub fn observe(&mut self, observation: &str) {
        let now = chrono::Utc::now();

        // Simple keyword-based learning (in production, the LLM would do this)
        let obs_lower = observation.to_lowercase();

        if obs_lower.contains("i'm a ") || obs_lower.contains("i am a ") {
            // Try to detect technical level
            if obs_lower.contains("developer") || obs_lower.contains("engineer") || obs_lower.contains("programmer") {
                self.user.technical_level = "advanced".to_string();
            }
        }

        if obs_lower.contains("my name is ") || obs_lower.contains("call me ") {
            // Try to extract name
            for prefix in ["my name is ", "call me "] {
                if let Some(idx) = obs_lower.find(prefix) {
                    let after = &observation[idx + prefix.len()..];
                    let name = after.split(|c: char| !c.is_alphabetic() && c != '-').next().unwrap_or("").trim();
                    if !name.is_empty() && name.len() < 50 {
                        self.user.name = name.to_string();
                        self.user.what_to_call_them = name.to_string();
                        info!("Learned user name: {}", name);
                    }
                }
            }
        }

        // Add as note
        self.user.notes.push(observation.to_string());
        self.user.updated_at = now;

        // Keep notes manageable
        if self.user.notes.len() > 100 {
            self.user.notes = self.user.notes.split_off(self.user.notes.len() - 100);
        }
    }

    pub fn record_topic(&mut self, topic: impl Into<String>) {
        let topic = topic.into();
        if !self.topics.contains(&topic) {
            self.topics.push(topic);
            if self.topics.len() > 50 {
                self.topics = self.topics.split_off(self.topics.len() - 50);
            }
        }
    }
}

impl Default for FullContext {
    fn default() -> Self {
        Self::new()
    }
}

fn push_section(buf: &mut String, title: &str, items: &[String]) {
    buf.push_str(&format!("# {}\n", title));
    for item in items {
        buf.push_str(&format!("- {}\n", item));
    }
    buf.push('\n');
}

fn push_json_section(buf: &mut String, title: &str, val: &serde_json::Value) {
    if val.is_object() && !val.as_object().unwrap().is_empty() {
        buf.push_str(&format!("# {}\n", title));
        if let Ok(pretty) = serde_json::to_string_pretty(val) {
            buf.push_str(&pretty);
            buf.push('\n');
        }
        buf.push('\n');
    }
}
