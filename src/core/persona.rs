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

impl Persona {
    /// Path to the persisted persona ("SOUL") file under the data dir.
    pub fn persona_path(data_dir: &str) -> PathBuf {
        PathBuf::from(format!("{}/persona.json", data_dir))
    }

    /// Load the persisted persona, falling back to the default if absent/invalid.
    pub fn load_or_default(data_dir: &str) -> Self {
        let path = Self::persona_path(data_dir);
        match std::fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(p) => {
                    debug!("Loaded persona from {}", path.display());
                    p
                }
                Err(e) => {
                    info!("persona.json present but unreadable ({e}); using default");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// Persist the persona to `<data_dir>/persona.json`.
    pub fn save(&self, data_dir: &str) -> anyhow::Result<()> {
        let path = Self::persona_path(data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        info!("Saved persona to {}", path.display());
        Ok(())
    }
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

impl UserModel {
    /// Path to the persisted user-model file under the data dir.
    pub fn user_model_path(data_dir: &str) -> PathBuf {
        PathBuf::from(format!("{}/user_model.json", data_dir))
    }

    /// Load the persisted user model, falling back to the default if absent/invalid.
    pub fn load_or_default(data_dir: &str) -> Self {
        let path = Self::user_model_path(data_dir);
        match std::fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(m) => {
                    debug!("Loaded user model from {}", path.display());
                    m
                }
                Err(e) => {
                    info!("user_model.json present but unreadable ({e}); using default");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// Persist the user model to `<data_dir>/user_model.json`.
    pub fn save(&self, data_dir: &str) -> anyhow::Result<()> {
        let path = Self::user_model_path(data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        debug!("Saved user model to {}", path.display());
        Ok(())
    }

    /// True when nothing has been learned yet (matches a fresh `default()`):
    /// no name learned, default technical level, and no interests/projects/notes.
    /// Used to decide whether to hydrate from disk without clobbering an
    /// already-populated in-memory model.
    pub fn is_empty(&self) -> bool {
        self.name == "User"
            && self.what_to_call_them == "friend"
            && self.timezone == "UTC"
            && self.language == "English"
            && self.technical_level == "intermediate"
            && self.communication_style == "balanced"
            && self.interests.is_empty()
            && self.projects.is_empty()
            && self.notes.is_empty()
            && self.preferences.as_object().map_or(true, |o| o.is_empty())
    }
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

/// Durable user insights extracted by the LLM from recent conversation.
/// Every field is optional + `#[serde(default)]` so a partial or `{}` extraction
/// merges cleanly. This is the structured contract we ask the model to emit.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserInsights {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub what_to_call_them: Option<String>,
    #[serde(default)]
    pub technical_level: Option<String>,
    #[serde(default)]
    pub communication_style: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub interests: Vec<String>,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Parse a `UserInsights` JSON object out of a (possibly fenced / chatty) model
/// reply. Tolerant: grabs the outermost `{ .. }` span before deserializing.
pub fn parse_insights(raw: &str) -> anyhow::Result<UserInsights> {
    let json = extract_json_object(raw)
        .ok_or_else(|| anyhow::anyhow!("no JSON object found in model output"))?;
    let insights: UserInsights = serde_json::from_str(json)?;
    Ok(insights)
}

/// Outermost `{...}` slice, ignoring code fences / prose around it.
fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

/// Trim a candidate scalar; reject empty / sentinel / over-long values so the
/// model can't poison a field with prose or "unknown".
fn clean_scalar(v: &Option<String>) -> Option<String> {
    let s = v.as_deref()?.trim();
    if s.is_empty() || s.len() > 100 {
        return None;
    }
    match s.to_lowercase().as_str() {
        "unknown" | "n/a" | "none" | "null" => None,
        _ => Some(s.to_string()),
    }
}

/// Append new items not already present (case-insensitive), trimming blanks and
/// capping to `cap` most-recent. Returns true if `dst` changed.
fn merge_string_list(dst: &mut Vec<String>, src: &[String], cap: usize) -> bool {
    let mut changed = false;
    for item in src {
        let item = item.trim();
        if item.is_empty() || item.len() > 200 {
            continue;
        }
        if !dst.iter().any(|e| e.eq_ignore_ascii_case(item)) {
            dst.push(item.to_string());
            changed = true;
        }
    }
    if dst.len() > cap {
        let drop = dst.len() - cap;
        dst.drain(..drop);
        changed = true;
    }
    changed
}

impl UserModel {
    /// Conservatively merge LLM-extracted insights into this model. Scalars are
    /// overwritten only when the extraction supplies a meaningful value; list
    /// fields are unioned (case-insensitive, deduped) and capped. Returns true
    /// if anything actually changed — pure/sync, so it is unit-testable without
    /// a network call.
    pub fn merge_insights(&mut self, insights: &UserInsights) -> bool {
        let mut changed = false;

        if let Some(v) = clean_scalar(&insights.name) {
            if self.name != v {
                self.name = v.clone();
                changed = true;
            }
            // Adopt as the address term only while it's still the placeholder.
            if self.what_to_call_them == "friend" && self.what_to_call_them != v {
                self.what_to_call_them = v;
                changed = true;
            }
        }
        if let Some(v) = clean_scalar(&insights.what_to_call_them) {
            if self.what_to_call_them != v {
                self.what_to_call_them = v;
                changed = true;
            }
        }
        if let Some(v) = clean_scalar(&insights.technical_level) {
            if self.technical_level != v {
                self.technical_level = v;
                changed = true;
            }
        }
        if let Some(v) = clean_scalar(&insights.communication_style) {
            if self.communication_style != v {
                self.communication_style = v;
                changed = true;
            }
        }
        if let Some(v) = clean_scalar(&insights.timezone) {
            if self.timezone != v {
                self.timezone = v;
                changed = true;
            }
        }
        if let Some(v) = clean_scalar(&insights.language) {
            if self.language != v {
                self.language = v;
                changed = true;
            }
        }

        changed |= merge_string_list(&mut self.interests, &insights.interests, 50);
        changed |= merge_string_list(&mut self.projects, &insights.projects, 50);
        changed |= merge_string_list(&mut self.notes, &insights.notes, 100);

        if changed {
            self.updated_at = chrono::Utc::now();
        }
        changed
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

    /// LLM-based user modeling (Honcho-style), augmenting the naive keyword
    /// `observe()`. Makes ONE chat-completions call over the recent transcript,
    /// extracts durable facts as JSON, and merges them into `self.user`. Returns
    /// `Ok(true)` when the model changed (so the caller knows to persist) —
    /// persistence stays the caller's job, matching today's
    /// `ctx.user.save(&workspace_dir)` path in `agent.rs`.
    ///
    /// Best-effort by contract: callers should `.ok()`/log the error and never
    /// fail a turn on it.
    pub async fn observe_llm(
        &mut self,
        recent: &[super::Message],
        config: &crate::config::Config,
        client: &reqwest::Client,
    ) -> anyhow::Result<bool> {
        let transcript = render_transcript(recent);
        if transcript.is_empty() {
            return Ok(false);
        }

        let current = serde_json::json!({
            "name": self.user.name,
            "what_to_call_them": self.user.what_to_call_them,
            "technical_level": self.user.technical_level,
            "communication_style": self.user.communication_style,
            "timezone": self.user.timezone,
            "language": self.user.language,
            "interests": self.user.interests,
            "projects": self.user.projects,
        });

        let system = "You maintain a durable profile of a user for a personal AI assistant. \
From the conversation, extract ONLY durable, high-confidence facts about the user: their name, \
what to call them, technical_level (beginner/intermediate/advanced/expert), communication_style \
(concise/detailed/technical/casual), timezone, preferred language, lasting interests, ongoing \
projects, and short notes about goals or habits. Ignore one-off task details and anything you are \
unsure about. Reply with ONE JSON object and nothing else, using only these optional keys: name, \
what_to_call_them, technical_level, communication_style, timezone, language, interests (array of \
short strings), projects (array of short strings), notes (array of short strings). Omit any key \
you cannot fill confidently. Return {} if nothing durable is present.";

        let user = format!(
            "Existing profile (may be incomplete or default):\n{}\n\nRecent conversation:\n{}\n\n\
Return the JSON object of new or refined durable facts.",
            serde_json::to_string_pretty(&current).unwrap_or_default(),
            transcript,
        );

        let messages = vec![
            serde_json::json!({ "role": "system", "content": system }),
            serde_json::json!({ "role": "user", "content": user }),
        ];

        let (api_base, api_key, model) = crate::config::resolve_provider(config, "text");
        let raw =
            crate::core::agent::call_llm_raw(client, api_base, api_key, model, &messages).await?;

        let insights = parse_insights(&raw)?;
        Ok(self.user.merge_insights(&insights))
    }
}

/// Render user/assistant turns as a compact, bounded transcript (skips
/// system/tool noise). Shared shape with the session-summary writer.
fn render_transcript(messages: &[super::Message]) -> String {
    let mut out = String::new();
    for m in messages {
        let who = match m.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            _ => continue,
        };
        let body = m.content.trim();
        if body.is_empty() {
            continue;
        }
        let body: String = body.chars().take(2000).collect();
        out.push_str(who);
        out.push_str(": ");
        out.push_str(&body);
        out.push('\n');
    }
    out.trim().to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a unique temp data dir and clean it up when dropped.
    struct TempDir(String);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            let unique = format!(
                "oa_usermodel_{}_{}_{}",
                tag,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            p.push(unique);
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p.to_string_lossy().into_owned())
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn user_model_save_load_round_trip() {
        let dir = TempDir::new("roundtrip");

        let mut model = UserModel::default();
        model.name = "Sam".to_string();
        model.what_to_call_them = "Sam".to_string();
        model.technical_level = "advanced".to_string();
        model.interests = vec!["rust".to_string(), "music".to_string()];
        model.projects = vec!["openAssistant".to_string()];
        model.notes = vec!["likes concise answers".to_string()];

        model.save(&dir.0).expect("save should succeed");

        let loaded = UserModel::load_or_default(&dir.0);
        assert_eq!(loaded.name, "Sam");
        assert_eq!(loaded.what_to_call_them, "Sam");
        assert_eq!(loaded.technical_level, "advanced");
        assert_eq!(loaded.interests, vec!["rust".to_string(), "music".to_string()]);
        assert_eq!(loaded.projects, vec!["openAssistant".to_string()]);
        assert_eq!(loaded.notes, vec!["likes concise answers".to_string()]);
    }

    #[test]
    fn load_or_default_when_absent() {
        let dir = TempDir::new("absent");
        // No file written — should fall back to default and be empty.
        let loaded = UserModel::load_or_default(&dir.0);
        assert!(loaded.is_empty());
    }

    #[test]
    fn parse_insights_extracts_json_from_fenced_text() {
        let raw = "Here is the profile:\n```json\n\
{ \"name\": \"Ada\", \"technical_level\": \"expert\", \
\"interests\": [\"compilers\", \"Rust\"], \"projects\": [\"openAssistant\"], \
\"notes\": [\"prefers terse answers\"] }\n```\ndone.";
        let insights = parse_insights(raw).expect("should pull JSON out of fenced prose");

        let mut model = UserModel::default();
        assert!(model.merge_insights(&insights), "first merge changes the model");
        assert_eq!(model.name, "Ada");
        assert_eq!(model.what_to_call_them, "Ada", "name adopted over the 'friend' placeholder");
        assert_eq!(model.technical_level, "expert");
        assert!(model.interests.iter().any(|i| i == "Rust"));
        assert!(model.projects.iter().any(|p| p == "openAssistant"));
        assert!(model.notes.iter().any(|n| n.contains("terse")));

        // Idempotent: merging identical insights again is a no-op.
        assert!(!model.merge_insights(&insights), "re-merging identical insights changes nothing");
    }

    #[test]
    fn merge_insights_rejects_empty_and_sentinel_values() {
        let insights = parse_insights(
            r#"{"name":"unknown","technical_level":"   ","interests":[""],"language":"n/a"}"#,
        )
        .unwrap();
        let mut model = UserModel::default();
        assert!(!model.merge_insights(&insights), "no usable fields => no change");
        assert_eq!(model.name, "User");
        assert_eq!(model.technical_level, "intermediate");
        assert_eq!(model.language, "English");
    }

    #[test]
    fn merge_insights_unions_lists_without_duplicates() {
        let mut model = UserModel::default();
        model.interests = vec!["Rust".to_string()];
        let insights = parse_insights(r#"{"interests":["rust","music"]}"#).unwrap();
        assert!(model.merge_insights(&insights));
        // case-insensitive dedupe: "rust" not re-added, "music" appended.
        assert_eq!(model.interests, vec!["Rust".to_string(), "music".to_string()]);
    }

    #[test]
    fn is_empty_reflects_learning() {
        let model = UserModel::default();
        assert!(model.is_empty(), "fresh default should be empty");

        let mut ctx = FullContext::new();
        ctx.observe("my name is Sam");
        assert!(
            !ctx.user.is_empty(),
            "after learning a name + note the model is no longer empty"
        );
    }
}
