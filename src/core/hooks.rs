// src/core/hooks.rs
//! Hooks system — event-driven automation (OpenClaw-style)
//! Small scripts/configs that run when events fire inside the gateway

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, debug};

/// Events that can trigger hooks
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum HookEvent {
    SessionNew,
    SessionEnd,
    MessageReceived,
    MessageSent,
    ToolCalled,
    ToolReturned,
    Error,
    UserJoined,
    UserLeft,
    CronFired,
    SkillLoaded,
    MemoryUpdated,
    AgentBoot,
    AgentShutdown,
}

/// Action to take when a hook fires
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookAction {
    /// Run a shell command
    Shell { command: String },
    /// Save a note to daily memory
    SaveNote { template: String },
    /// Send a message to a channel
    SendMessage { channel: String, message: String },
    /// Trigger another hook (chain)
    ChainHook { hook_name: String },
    /// Run a skill
    RunSkill { skill_name: String },
    /// Webhook HTTP call
    Webhook { url: String, body: String },
    /// No-op (for logging only)
    Log { message: String },
}

/// A single hook definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub name: String,
    pub event: HookEvent,
    pub actions: Vec<HookAction>,
    pub enabled: bool,
    pub description: String,
}

/// Hook engine — registers and fires hooks
#[derive(Debug, Default)]
pub struct HookEngine {
    hooks: HashMap<HookEvent, Vec<Hook>>,
}

impl HookEngine {
    pub fn new() -> Self {
        let mut engine = Self::default();
        engine.load_defaults();
        engine
    }

    /// Register a hook
    pub fn register(&mut self, hook: Hook) {
        debug!("Registering hook: {} for {:?}", hook.name, hook.event);
        self.hooks.entry(hook.event.clone()).or_default().push(hook);
    }

    /// Fire an event — runs all matching hooks
    pub async fn fire(&self, event: &HookEvent, context: &HookContext) -> Vec<HookResult> {
        let mut results = Vec::new();

        if let Some(hooks) = self.hooks.get(event) {
            for hook in hooks {
                if !hook.enabled { continue; }
                info!("Firing hook: {} for {:?}", hook.name, event);

                for action in &hook.actions {
                    let result = self.execute_action(action, context);
                    results.push(HookResult {
                        hook_name: hook.name.clone(),
                        action: format!("{:?}", action),
                        success: result.is_ok(),
                        error: result.err().map(|e| e.to_string()),
                    });
                }
            }
        }

        results
    }

    fn execute_action(&self, action: &HookAction, ctx: &HookContext) -> Result<()> {
        match action {
            HookAction::Shell { command } => {
                let cmd = interpolate(command, ctx);
                debug!("Hook running shell: {}", cmd);
                std::process::Command::new("bash")
                    .arg("-c")
                    .arg(&cmd)
                    .output()?;
            }
            HookAction::SaveNote { template } => {
                let note = interpolate(template, ctx);
                debug!("Hook saving note: {}", note);
                // The actual save happens in the daily memory system
            }
            HookAction::SendMessage { channel, message } => {
                let msg = interpolate(message, ctx);
                info!("Hook sending to {}: {}", channel, &msg[..msg.len().min(100)]);
            }
            HookAction::Log { message } => {
                let msg = interpolate(message, ctx);
                info!("[HOOK] {}", msg);
            }
            HookAction::Webhook { url, body } => {
                let url = interpolate(url, ctx);
                let _body = interpolate(body, ctx);
                debug!("Hook webhook: {}", url);
            }
            HookAction::RunSkill { skill_name } => {
                info!("Hook running skill: {}", skill_name);
            }
            HookAction::ChainHook { hook_name } => {
                info!("Hook chaining to: {}", hook_name);
            }
        }
        Ok(())
    }

    pub fn list_hooks(&self) -> Vec<&Hook> {
        self.hooks.values().flat_map(|v| v.iter()).collect()
    }

    pub fn enable_hook(&mut self, name: &str, enabled: bool) {
        for hooks in self.hooks.values_mut() {
            for hook in hooks {
                if hook.name == name {
                    hook.enabled = enabled;
                }
            }
        }
    }

    fn load_defaults(&mut self) {
        // Session end → distill memories
        self.register(Hook {
            name: "session-distill".to_string(),
            event: HookEvent::SessionEnd,
            actions: vec![
                HookAction::Log { message: "Session ended. Consider distilling memories.".to_string() },
            ],
            enabled: true,
            description: "Triggered when a session ends — prompts memory distillation".to_string(),
        });

        // Error → log and notify
        self.register(Hook {
            name: "error-logger".to_string(),
            event: HookEvent::Error,
            actions: vec![
                HookAction::Log { message: "Error occurred: {{error_message}}".to_string() },
            ],
            enabled: true,
            description: "Triggered on errors — logs for debugging".to_string(),
        });

        // New memory saved → update search index
        self.register(Hook {
            name: "memory-index".to_string(),
            event: HookEvent::MemoryUpdated,
            actions: vec![
                HookAction::Log { message: "Memory updated: {{content}}".to_string() },
            ],
            enabled: true,
            description: "Triggered when memory is updated".to_string(),
        });

        // Agent boot → workspace init
        self.register(Hook {
            name: "boot-init".to_string(),
            event: HookEvent::AgentBoot,
            actions: vec![
                HookAction::Log { message: "Agent booted. Workspace: {{workspace_dir}}".to_string() },
            ],
            enabled: true,
            description: "Triggered when agent starts".to_string(),
        });
    }
}

/// Context available to hook templates
#[derive(Debug, Clone, Serialize)]
pub struct HookContext {
    pub workspace_dir: String,
    pub channel: String,
    pub user_id: String,
    pub user_name: String,
    pub session_id: String,
    pub message_count: usize,
    pub error_message: Option<String>,
    pub content: Option<String>,
    pub timestamp: String,
}

impl HookContext {
    pub fn new(workspace_dir: &str) -> Self {
        Self {
            workspace_dir: workspace_dir.to_string(),
            channel: "cli".to_string(),
            user_id: "local".to_string(),
            user_name: "User".to_string(),
            session_id: uuid::Uuid::new_v4().to_string(),
            message_count: 0,
            error_message: None,
            content: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookResult {
    pub hook_name: String,
    pub action: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Simple template interpolation
fn interpolate(template: &str, ctx: &HookContext) -> String {
    template
        .replace("{{workspace_dir}}", &ctx.workspace_dir)
        .replace("{{channel}}", &ctx.channel)
        .replace("{{user_id}}", &ctx.user_id)
        .replace("{{user_name}}", &ctx.user_name)
        .replace("{{session_id}}", &ctx.session_id)
        .replace("{{timestamp}}", &ctx.timestamp)
        .replace("{{error_message}}", &ctx.error_message.clone().unwrap_or_default())
        .replace("{{content}}", &ctx.content.clone().unwrap_or_default())
        .replace("{{message_count}}", &ctx.message_count.to_string())
}
