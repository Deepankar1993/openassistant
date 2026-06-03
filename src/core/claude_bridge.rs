// src/core/claude_bridge.rs
//! Bridge to a locally-installed **Claude Code CLI** (`claude`).
//!
//! Runs `claude -p --output-format json` non-interactively, feeding the prompt
//! over stdin, and parses the structured result (`result` + `session_id`).
//! Session continuity is achieved by passing back the previous `session_id` via
//! `--resume`, so a Discord thread (or any conversation) maps to one persistent
//! Claude Code session operating on a real project directory.
//!
//! This is how openAssistant becomes a human-friendly front-end (Discord, CLI,
//! web) for the same Claude Code the user runs locally.

use anyhow::Result;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{debug, warn};

use crate::config::ClaudeBridgeConfig;

#[derive(Debug, Clone)]
pub struct ClaudeResult {
    pub text: String,
    pub session_id: Option<String>,
    pub is_error: bool,
    pub cost_usd: Option<f64>,
}

/// Who initiated a bridge call. The **local operator** (the `openassistant
/// claude` CLI on this machine) is trusted with full autonomy; **remote**
/// callers (Discord authors, LLM tool-loop) are capped to a non-bypass
/// permission mode and can never trigger `--dangerously-skip-permissions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeOrigin {
    Operator,
    Remote,
}

#[derive(Debug, Clone)]
pub struct ClaudeBridge {
    bin: String,
    workspace: String,
    model: String,
    permission_mode: String,
    skip_permissions: bool,
    append_system_prompt: String,
    timeout_secs: u64,
    origin: BridgeOrigin,
}

impl ClaudeBridge {
    pub fn from_config(cfg: &ClaudeBridgeConfig, data_dir: &str) -> Self {
        let workspace = if cfg.workspace.trim().is_empty() {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| data_dir.to_string())
        } else {
            cfg.workspace.clone()
        };
        Self {
            bin: if cfg.bin.trim().is_empty() { "claude".to_string() } else { cfg.bin.clone() },
            workspace,
            model: cfg.model.clone(),
            permission_mode: cfg.permission_mode.clone(),
            skip_permissions: cfg.skip_permissions,
            append_system_prompt: cfg.append_system_prompt.clone(),
            timeout_secs: if cfg.timeout_secs == 0 { 300 } else { cfg.timeout_secs },
            // Safe by default: only the local-operator CLI path opts into trust.
            origin: BridgeOrigin::Remote,
        }
    }

    /// Mark this bridge as operator-initiated (local `openassistant claude` CLI),
    /// permitting `--dangerously-skip-permissions` / `bypassPermissions`.
    pub fn operator(mut self) -> Self {
        self.origin = BridgeOrigin::Operator;
        self
    }

    /// Override / extend the appended system prompt (used to inject persona +
    /// a human conversational tone).
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.append_system_prompt = prompt.into();
        self
    }

    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    /// Check that the binary responds to `--version`.
    pub async fn available(&self) -> bool {
        Command::new(&self.bin)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn build_args(&self, resume: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "-p".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ];
        if !self.model.trim().is_empty() {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }
        if !self.append_system_prompt.trim().is_empty() {
            args.push("--append-system-prompt".to_string());
            args.push(self.append_system_prompt.clone());
        }
        // Permission policy is origin-aware. The nuclear bypass is reachable
        // ONLY from the local operator CLI; remote callers (Discord/LLM tool)
        // are capped to a non-bypass mode regardless of config, so an allowlisted
        // chat author can't escalate to full unsandboxed autonomy.
        let operator = self.origin == BridgeOrigin::Operator;
        if self.skip_permissions && operator {
            args.push("--dangerously-skip-permissions".to_string());
        } else {
            let mode = effective_permission_mode(&self.permission_mode, operator);
            if !mode.is_empty() {
                args.push("--permission-mode".to_string());
                args.push(mode);
            }
        }
        if let Some(sid) = resume {
            if !sid.is_empty() {
                args.push("--resume".to_string());
                args.push(sid.to_string());
            }
        }
        args
    }

    /// Run a prompt through `claude`. `resume` is a prior session id for
    /// continuity (None starts a new session).
    pub async fn run(&self, prompt: &str, resume: Option<&str>) -> Result<ClaudeResult> {
        let args = self.build_args(resume);
        debug!("claude bridge: {} {:?} (cwd={})", self.bin, args, self.workspace);

        let mut child = Command::new(&self.bin)
            .args(&args)
            .current_dir(&self.workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to launch '{}': {} (is Claude Code installed / on PATH?)", self.bin, e))?;

        // Feed the prompt over stdin, then close it.
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.shutdown().await.ok();
        }

        let output = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("claude timed out after {}s", self.timeout_secs))??;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() && stdout.trim().is_empty() {
            anyhow::bail!("claude exited with {}: {}", output.status, stderr.trim());
        }

        Ok(parse_result(&stdout, &stderr))
    }
}

/// Resolve the `--permission-mode` to actually use. Empty config ⇒ `acceptEdits`.
/// For non-operator (remote) callers, the full-bypass mode is downgraded so a
/// Discord/LLM-driven prompt can never bypass all permission checks.
fn effective_permission_mode(configured: &str, operator: bool) -> String {
    let m = configured.trim();
    if m.is_empty() {
        return "acceptEdits".to_string();
    }
    if !operator && m.eq_ignore_ascii_case("bypassPermissions") {
        return "acceptEdits".to_string();
    }
    m.to_string()
}

/// Parse `claude --output-format json` stdout. Falls back to raw text if the
/// payload isn't the expected JSON object.
fn parse_result(stdout: &str, stderr: &str) -> ClaudeResult {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        let text = json["result"]
            .as_str()
            .or_else(|| json["text"].as_str())
            .unwrap_or("")
            .to_string();
        let session_id = json["session_id"].as_str().map(|s| s.to_string());
        let is_error = json["is_error"].as_bool().unwrap_or(false);
        let cost_usd = json["total_cost_usd"].as_f64();
        if !text.is_empty() || session_id.is_some() {
            return ClaudeResult { text, session_id, is_error, cost_usd };
        }
    }
    // Not JSON (or empty) — return whatever we got.
    let raw = stdout.trim();
    if raw.is_empty() {
        warn!("claude returned no parseable output; stderr: {}", stderr.trim());
    }
    ClaudeResult {
        text: if raw.is_empty() { stderr.trim().to_string() } else { raw.to_string() },
        session_id: None,
        is_error: raw.is_empty(),
        cost_usd: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge() -> ClaudeBridge {
        ClaudeBridge::from_config(&ClaudeBridgeConfig::default(), ".")
    }

    #[test]
    fn args_include_json_and_permission_mode() {
        let args = bridge().build_args(None);
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--permission-mode".to_string()));
        assert!(args.contains(&"acceptEdits".to_string()));
        assert!(!args.contains(&"--resume".to_string()));
    }

    #[test]
    fn args_resume_when_session_given() {
        let args = bridge().build_args(Some("sess-123"));
        let i = args.iter().position(|a| a == "--resume").expect("has --resume");
        assert_eq!(args[i + 1], "sess-123");
    }

    #[test]
    fn skip_permissions_honored_only_for_operator() {
        let mut cfg = ClaudeBridgeConfig::default();
        cfg.skip_permissions = true;

        // Operator (local CLI) → full bypass allowed.
        let op = ClaudeBridge::from_config(&cfg, ".").operator().build_args(None);
        assert!(op.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(!op.contains(&"--permission-mode".to_string()));

        // Remote (Discord / LLM tool) → bypass refused, capped to a mode.
        let remote = ClaudeBridge::from_config(&cfg, ".").build_args(None);
        assert!(!remote.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(remote.contains(&"--permission-mode".to_string()));
    }

    #[test]
    fn remote_downgrades_bypass_mode_but_operator_keeps_it() {
        let mut cfg = ClaudeBridgeConfig::default();
        cfg.permission_mode = "bypassPermissions".to_string();

        let remote = ClaudeBridge::from_config(&cfg, ".").build_args(None);
        let ri = remote.iter().position(|a| a == "--permission-mode").unwrap();
        assert_eq!(remote[ri + 1], "acceptEdits", "remote bypass is downgraded");

        let op = ClaudeBridge::from_config(&cfg, ".").operator().build_args(None);
        let oi = op.iter().position(|a| a == "--permission-mode").unwrap();
        assert_eq!(op[oi + 1], "bypassPermissions", "operator keeps configured mode");
    }

    #[test]
    fn parses_json_result() {
        let json = r#"{"type":"result","is_error":false,"result":"hello there","session_id":"abc","total_cost_usd":0.01}"#;
        let r = parse_result(json, "");
        assert_eq!(r.text, "hello there");
        assert_eq!(r.session_id.as_deref(), Some("abc"));
        assert!(!r.is_error);
    }

    #[test]
    fn falls_back_to_raw_text() {
        let r = parse_result("just plain text", "");
        assert_eq!(r.text, "just plain text");
        assert!(r.session_id.is_none());
    }
}
