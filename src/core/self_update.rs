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

    /// Check if an update is available (compare with crates.io or git)
    pub async fn check_update(&self) -> Result<Option<String>> {
        // Check current version
        let current = env!("CARGO_PKG_VERSION");

        // Try to get latest from crates.io
        let client = reqwest::Client::new();
        match client
            .get(format!("https://crates.io/api/v1/crates/open-assistant"))
            .header("User-Agent", format!("openAssistant/{}", current))
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(latest) = json["crate"]["newest_version"].as_str() {
                        if latest != current {
                            info!("Update available: {} → {}", current, latest);
                            return Ok(Some(latest.to_string()));
                        }
                    }
                }
            }
            Err(e) => {
                debug!("Could not check crates.io: {}", e);
            }
        }

        Ok(None)
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
    let output = tokio::task::spawn_blocking({
        let cwd = cwd.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let cmd = cmd.to_string();
        move || {
            std::process::Command::new(&cmd)
                .args(&args)
                .current_dir(&cwd)
                .output()
        }
    }).await.unwrap_or(Ok(std::process::Output {
        status: std::process::ExitStatus::default(),
        stdout: Vec::new(),
        stderr: vec![],
    }))?;

    if !output.status.success() {
        warn!("Command failed: {} {:?}: {}", cmd, args,
            String::from_utf8_lossy(&output.stderr));
    }

    Ok(output)
}
