// src/core/hooks.rs
//! Hooks system — event-driven shell command callbacks (Claude Code-style)
//! Hooks are defined in .claude/hooks/hooks.json and execute shell commands
//! at key agent lifecycle events.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, debug, warn};

// ─── Hook Events ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum HookEvent {
    #[serde(rename = "session_start")]
    SessionStart,
    #[serde(rename = "user_prompt_submit")]
    UserPromptSubmit,
    #[serde(rename = "pre_tool_use")]
    PreToolUse,
    #[serde(rename = "post_tool_use")]
    PostToolUse,
    #[serde(rename = "post_tool_use_failure")]
    PostToolUseFailure,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "subagent_stop")]
    SubagentStop,
    #[serde(rename = "pre_compact")]
    PreCompact,
    #[serde(rename = "notification")]
    Notification,
    #[serde(rename = "message_display")]
    MessageDisplay,
}

// ─── Hook Definition (from hooks.json) ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDefinition {
    /// Which event triggers this hook
    pub event: HookEvent,
    /// Shell command to execute
    pub command: String,
    /// Human-readable description
    pub description: String,
    /// Working directory (defaults to workspace root)
    pub working_dir: Option<String>,
    /// Timeout in seconds (default 30)
    pub timeout_seconds: Option<u64>,
    /// Environment variables to set
    pub env: Option<HashMap<String, String>>,
    /// Whether this hook is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// For PreToolUse security hooks: if the hook fails to spawn or times out,
    /// treat that as a BLOCK rather than allowing the tool (fail-closed).
    /// Default false (fail-open) for non-security hooks.
    #[serde(default)]
    pub fail_closed: bool,
}

/// The shell used to run hook commands, per platform. Hook authors write
/// commands in this shell's syntax (PowerShell on Windows, bash elsewhere).
pub(crate) fn hook_shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("powershell", "-Command")
    } else {
        ("bash", "-c")
    }
}

fn default_enabled() -> bool {
    true
}

// ─── Hook Context (passed to hooks via stdin as JSON) ─────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    pub session_id: String,
    pub workspace_dir: String,
    pub event: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_output: Option<String>,
    pub user_message: Option<String>,
    pub assistant_message: Option<String>,
    pub timestamp: String,
}

// ─── Hook Result ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResult {
    pub hook_event: String,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    /// If the hook wants to block a tool (PreToolUse only)
    pub block: bool,
    /// Human-readable reason for a block (from the hook's `{"reason": ...}`).
    pub block_reason: Option<String>,
    /// If the hook wants to modify the tool input
    pub modified_input: Option<serde_json::Value>,
}

// ─── Hook Engine ──────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct HookEngine {
    hooks: Vec<HookDefinition>,
}

impl HookEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load hooks from .claude/hooks/hooks.json
    pub fn load_from_file(path: &str) -> Result<Self> {
        let mut engine = Self::new();
        engine.load_hooks(path)?;
        Ok(engine)
    }

    /// Auto-load from workspace directory
    pub fn load_from_workspace(workspace_dir: &str) -> Result<Self> {
        let hooks_path = format!("{}/.claude/hooks/hooks.json", workspace_dir);
        Self::load_from_file(&hooks_path)
    }

    fn load_hooks(&mut self, path: &str) -> Result<()> {
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            debug!("Hooks file not found: {}", path);
            return Ok(());
        }

        let content = std::fs::read_to_string(&path_buf)?;
        let hooks: Vec<HookDefinition> = serde_json::from_str(&content)?;
        info!("Loaded {} hooks from {}", hooks.len(), path);
        self.hooks = hooks;
        Ok(())
    }

    pub fn register(&mut self, hook: HookDefinition) {
        self.hooks.push(hook);
    }

    /// Fire all hooks matching the given event
    pub async fn fire(&self, event: &HookEvent, ctx: &HookContext) -> Vec<HookResult> {
        let matching: Vec<&HookDefinition> = self.hooks
            .iter()
            .filter(|h| &h.event == event && h.enabled)
            .collect();

        let mut results = Vec::new();

        for hook in matching {
            let result = self.execute_hook(hook, ctx).await;
            results.push(result);
        }

        results
    }

    /// Execute a single hook — runs the shell command
    async fn execute_hook(&self, hook: &HookDefinition, ctx: &HookContext) -> HookResult {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(hook.timeout_seconds.unwrap_or(30));

        // Build the command in the platform shell.
        let (shell, flag) = hook_shell();
        let mut cmd = tokio::process::Command::new(shell);
        cmd.arg(flag).arg(&hook.command);

        // Set working directory
        if let Some(ref dir) = hook.working_dir {
            cmd.current_dir(dir);
        }

        // Set environment variables
        if let Some(ref env_vars) = hook.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        // Pass context as JSON via stdin
        let ctx_json = serde_json::to_string(ctx).unwrap_or_default();
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true); // don't orphan a hung hook if the timeout fires
        crate::core::proc::no_window(&mut cmd); // no console window flash on Windows

        debug!("Executing hook: {} (event: {:?})", hook.description, hook.event);

        // Execute with timeout
        let output = match tokio::time::timeout(timeout, async {
            let mut child = cmd.spawn()?;
            // Write context to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(ctx_json.as_bytes()).await;
            }
            child.wait_with_output().await
        }).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                // fail_closed hooks treat a spawn failure as a block.
                return HookResult {
                    hook_event: format!("{:?}", hook.event),
                    command: hook.command.clone(),
                    stdout: String::new(),
                    stderr: format!("Failed to execute hook: {}", e),
                    exit_code: -1,
                    duration_ms: start.elapsed().as_millis() as u64,
                    block: hook.fail_closed,
                    block_reason: hook.fail_closed.then(|| format!("hook failed to run: {}", e)),
                    modified_input: None,
                };
            }
            Err(_) => {
                warn!("Hook timed out after {:?}: {}", timeout, hook.description);
                return HookResult {
                    hook_event: format!("{:?}", hook.event),
                    command: hook.command.clone(),
                    stdout: String::new(),
                    stderr: format!("Hook timed out after {:?}", timeout),
                    exit_code: -2,
                    duration_ms: start.elapsed().as_millis() as u64,
                    block: hook.fail_closed,
                    block_reason: hook.fail_closed.then(|| "hook timed out".to_string()),
                    modified_input: None,
                };
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        // Parse hook response — hooks can return JSON to block/modify
        let (block, modified_input, block_reason) = if !stdout.trim().is_empty() {
            if let Ok(response) = serde_json::from_str::<HookResponse>(stdout.trim()) {
                (response.block, response.modified_input, response.reason)
            } else {
                (false, None, None)
            }
        } else {
            (false, None, None)
        };

        HookResult {
            hook_event: format!("{:?}", hook.event),
            command: hook.command.clone(),
            stdout,
            stderr,
            exit_code,
            duration_ms: start.elapsed().as_millis() as u64,
            block,
            block_reason,
            modified_input,
        }
    }

    pub fn list_hooks(&self) -> &[HookDefinition] {
        &self.hooks
    }

    pub fn enable_hook(&mut self, description: &str, enabled: bool) {
        for hook in &mut self.hooks {
            if hook.description == description {
                hook.enabled = enabled;
                info!("Hook '{}' enabled: {}", description, enabled);
                return;
            }
        }
    }
}

/// Hook response format (stdout from hook command)
#[derive(Debug, Deserialize)]
struct HookResponse {
    #[serde(default)]
    block: bool,
    modified_input: Option<serde_json::Value>,
    /// Optional human-readable reason for a block.
    reason: Option<String>,
}

// ─── Default Hooks ────────────────────────────────────────────────────

pub fn default_hooks() -> Vec<HookDefinition> {
    vec![
        HookDefinition {
            event: HookEvent::SessionStart,
            command: "echo 'Session started'".to_string(),
            description: "Log session start".to_string(),
            working_dir: None,
            timeout_seconds: Some(5),
            env: None,
            enabled: false, // Disabled by default
            fail_closed: false,
        },
        HookDefinition {
            event: HookEvent::PreToolUse,
            command: "echo '{}'".to_string(),
            description: "Pre-tool-use validation".to_string(),
            working_dir: None,
            timeout_seconds: Some(5),
            env: None,
            enabled: false,
            fail_closed: false,
        },
        HookDefinition {
            event: HookEvent::PostToolUse,
            command: "echo 'Tool completed'".to_string(),
            description: "Post-tool-use logging".to_string(),
            working_dir: None,
            timeout_seconds: Some(5),
            env: None,
            enabled: false,
            fail_closed: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_hooks_from_json() {
        let json = r#"[
            {
                "event": "session_start",
                "command": "echo 'hello'",
                "description": "Test hook",
                "timeout_seconds": 5,
                "enabled": true
            }
        ]"#;

        let hooks: Vec<HookDefinition> = serde_json::from_str(json).unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].description, "Test hook");
        assert!(hooks[0].enabled);
    }

    #[test]
    fn test_hook_response_parsing() {
        let json = r#"{"block": true, "modified_input": {"command": "safe_command"}, "reason": "nope"}"#;
        let response: HookResponse = serde_json::from_str(json).unwrap();
        assert!(response.block);
        assert!(response.modified_input.is_some());
        assert_eq!(response.reason.as_deref(), Some("nope"));
    }

    #[tokio::test]
    async fn fail_closed_hook_blocks_on_timeout() {
        // A security hook that times out must BLOCK when fail_closed (fail-closed),
        // not silently allow the tool. Uses the platform shell's sleep.
        let sleep = if cfg!(windows) { "Start-Sleep -Seconds 5" } else { "sleep 5" };
        let mut engine = HookEngine::new();
        engine.register(HookDefinition {
            event: HookEvent::PreToolUse,
            command: sleep.to_string(),
            description: "slow security hook".to_string(),
            working_dir: None,
            timeout_seconds: Some(1),
            env: None,
            enabled: true,
            fail_closed: true,
        });
        let ctx = HookContext {
            session_id: "s".into(), workspace_dir: ".".into(), event: "pre_tool_use".into(),
            tool_name: None, tool_input: None, tool_output: None, user_message: None,
            assistant_message: None, timestamp: "t".into(),
        };
        let results = engine.fire(&HookEvent::PreToolUse, &ctx).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].block, "fail_closed hook must block on timeout");
    }
}
