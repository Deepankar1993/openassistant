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

#[derive(Debug, Clone)]
pub struct ClaudeBridge {
    bin: String,
    workspace: String,
    model: String,
    permission_mode: String,
    skip_permissions: bool,
    append_system_prompt: String,
    timeout_secs: u64,
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
        }
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
        if self.skip_permissions {
            args.push("--dangerously-skip-permissions".to_string());
        } else if !self.permission_mode.trim().is_empty() {
            args.push("--permission-mode".to_string());
            args.push(self.permission_mode.clone());
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
    fn skip_permissions_replaces_mode() {
        let mut cfg = ClaudeBridgeConfig::default();
        cfg.skip_permissions = true;
        let b = ClaudeBridge::from_config(&cfg, ".");
        let args = b.build_args(None);
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(!args.contains(&"--permission-mode".to_string()));
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
