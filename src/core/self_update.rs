// src/core/self_update.rs
//! Self-update system — agent can update itself with user permission
//! Inspired by Hermes's `hermes update` + OpenClaw's self-management

use anyhow::Result;
use std::process::Output;
use tracing::{info, warn, debug};

/// Permission level for self-modification
#[derive(Debug, Clone, PartialEq)]
pub enum Permission {
    Allow,      // User has given blanket permission
    Ask,        // Ask before each action (default)
    Deny,       // Never allow
}

pub struct SelfUpdater {
    pub permission: Permission,
    pub workspace_dir: String,
}

impl SelfUpdater {
    pub fn new(workspace_dir: impl Into<String>) -> Self {
        Self {
            permission: Permission::Ask,
            workspace_dir: workspace_dir.into(),
        }
    }

    /// Check if an update is available on crates.io.
    ///
    /// DEAD PATH: `open-assistant` is not published to crates.io, so this used
    /// to always return `Ok(None)` — indistinguishable from "no update", a
    /// silent lie. It now fails explicitly so callers don't trust it; use the
    /// git-based [`check_pending`](Self::check_pending) instead.
    pub async fn check_update(&self) -> Result<Option<String>> {
        anyhow::bail!(
            "crates.io update check is unavailable: open-assistant is not published. \
             Use `openassistant update --check` (git-based) instead."
        )
    }

    /// List commits on the upstream branch that are not in the local HEAD.
    /// Runs `git fetch` then `git log HEAD..@{u} --oneline`. Returns the commit
    /// summary lines (empty = already up to date).
    pub async fn check_pending(&self) -> Result<Vec<String>> {
        let fetch = run_cmd("git", &["fetch", "--quiet"], &self.workspace_dir).await?;
        if !fetch.status.success() {
            return Err(anyhow::anyhow!(
                "git fetch failed: {}",
                String::from_utf8_lossy(&fetch.stderr).trim()
            ));
        }
        // `@{u}` (upstream of the current branch) is more robust than
        // `origin/HEAD`, which is often unset on fresh clones.
        let log = run_cmd("git", &["log", "HEAD..@{u}", "--oneline"], &self.workspace_dir).await?;
        if !log.status.success() {
            return Err(anyhow::anyhow!(
                "git log failed (no upstream tracking branch?): {}",
                String::from_utf8_lossy(&log.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&log.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }

    /// Whether the working tree has uncommitted changes (`git status --porcelain`).
    pub async fn is_dirty(&self) -> Result<bool> {
        let out = run_cmd("git", &["status", "--porcelain"], &self.workspace_dir).await?;
        if !out.status.success() {
            return Err(anyhow::anyhow!(
                "git status failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
    }

    /// Update the agent from source (git pull + cargo build)
    pub async fn update_from_source(&self) -> Result<bool> {
        if self.permission == Permission::Deny {
            warn!("Self-update denied by user preference");
            return Ok(false);
        }

        info!("Updating openAssistant from source...");

        // git pull
        let pull_output = run_cmd("git", &["pull", "--rebase"], &self.workspace_dir).await?;
        if !pull_output.status.success() {
            return Err(anyhow::anyhow!("git pull failed: {}",
                String::from_utf8_lossy(&pull_output.stderr)));
        }

        // Check if anything changed
        let stdout = String::from_utf8_lossy(&pull_output.stdout);
        if stdout.contains("Already up to date") {
            info!("Already at latest version");
            return Ok(false);
        }

        // cargo build
        info!("Building updated version...");
        let build_output = run_cmd("cargo", &["build", "--release"], &self.workspace_dir).await?;
        if !build_output.status.success() {
            return Err(anyhow::anyhow!("cargo build failed: {}",
                String::from_utf8_lossy(&build_output.stderr)));
        }

        info!("Update complete! Restart openAssistant to use the new version.");
        Ok(true)
    }

    /// Update a skill file
    pub async fn update_skill(&self, skill_name: &str, new_content: &str) -> Result<()> {
        let skill_path = format!("{}/skills/{}.md", self.workspace_dir, skill_name);
        tokio::fs::write(&skill_path, new_content).await?;
        info!("Updated skill: {}", skill_name);
        Ok(())
    }

    /// Add a new skill file
    pub async fn create_skill(&self, skill_name: &str, content: &str) -> Result<()> {
        tokio::fs::create_dir_all(format!("{}/skills", self.workspace_dir)).await?;
        let skill_path = format!("{}/skills/{}.md", self.workspace_dir, skill_name);
        tokio::fs::write(&skill_path, content).await?;
        info!("Created skill: {}", skill_name);
        Ok(())
    }

    /// Update the MEMORY.md file directly
    pub async fn update_memory(&self, content: &str) -> Result<()> {
        let path = format!("{}/MEMORY.md", self.workspace_dir);
        std::fs::write(&path, content)?;
        info!("Updated MEMORY.md");
        Ok(())
    }

    /// Run a system command safely with output capture
    pub async fn run_safe(&self, command: &str, args: &[&str]) -> Result<Output> {
        if self.permission == Permission::Deny {
            return Err(anyhow::anyhow!("Command execution denied by user preference"));
        }

        debug!("Running: {} {}", command, args.join(" "));
        run_cmd(command, args, &self.workspace_dir).await
    }

    /// List all skills
    pub async fn list_skills(&self) -> Result<Vec<String>> {
        let skills_dir = format!("{}/skills", self.workspace_dir);
        let mut skills = Vec::new();

        if let Ok(mut entries) = tokio::fs::read_dir(&skills_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".md") {
                        skills.push(name.trim_end_matches(".md").to_string());
                    }
                }
            }
        }

        Ok(skills)
    }

    /// Read a skill
    pub async fn read_skill(&self, skill_name: &str) -> Result<String> {
        let path = format!("{}/skills/{}.md", self.workspace_dir, skill_name);
        Ok(tokio::fs::read_to_string(&path).await.unwrap_or_default())
    }
}

async fn run_cmd(cmd: &str, args: &[&str], cwd: &str) -> Result<Output> {
    // Propagate both the JoinError (blocking task panicked) and the io::Error
    // (process failed to launch) with `??`. The previous `.unwrap_or(Ok(default
    // Output))` turned a panicked `cargo build` into a fake-success Output with
    // a default (success-looking) ExitStatus — a correctness hole.
    let output = tokio::task::spawn_blocking({
        let cwd = cwd.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let cmd = cmd.to_string();
        move || {
            let mut command = std::process::Command::new(&cmd);
            command.args(&args).current_dir(&cwd);
            crate::core::proc::no_window_std(&mut command); // no console window flash on Windows
            command.output()
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("'{}' task failed to join: {}", cmd, e))?
    .map_err(|e| anyhow::anyhow!("failed to launch '{}': {} (is it on PATH?)", cmd, e))?;

    if !output.status.success() {
        warn!("Command failed: {} {:?}: {}", cmd, args,
            String::from_utf8_lossy(&output.stderr));
    }

    Ok(output)
}
