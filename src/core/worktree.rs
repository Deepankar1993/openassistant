// src/core/worktree.rs
//! Git worktree isolation — create/remove/list isolated worktrees for sub-agents
//! Like Claude Code's `-w` / `--worktree` flag

use anyhow::Result;
use std::path::PathBuf;
use tracing::{info, debug};

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    pub commit: String,
}

#[derive(Debug, Default)]
pub struct WorktreeManager {
    base_dir: String,
    worktrees_dir: String,
}

impl WorktreeManager {
    pub fn new(base_dir: &str) -> Self {
        Self {
            base_dir: base_dir.to_string(),
            worktrees_dir: format!("{}/.claude/worktrees", base_dir),
        }
    }

    /// Create an isolated worktree for a sub-agent
    pub async fn create_worktree(&self, name: &str) -> Result<WorktreeInfo> {
        let worktree_path = PathBuf::from(&self.worktrees_dir).join(name);
        let branch_name = format!("agent-{}", name);

        info!("Creating worktree: {} at {:?}", name, worktree_path);

        // Ensure worktrees directory exists
        tokio::fs::create_dir_all(&self.worktrees_dir).await?;

        // Create the worktree
        let output = tokio::process::Command::new("git")
            .args(&["worktree", "add"])
            .arg(worktree_path.to_str().unwrap())
            .arg("-b")
            .arg(&branch_name)
            .current_dir(&self.base_dir)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If branch already exists, try without -b
            if stderr.contains("already exists") {
                let output2 = tokio::process::Command::new("git")
                    .args(&["worktree", "add"])
                    .arg(worktree_path.to_str().unwrap())
                    .arg(&branch_name)
                    .current_dir(&self.base_dir)
                    .output()
                    .await?;

                if !output2.status.success() {
                    return Err(anyhow::anyhow!(
                        "Failed to create worktree: {}",
                        String::from_utf8_lossy(&output2.stderr)
                    ));
                }
            } else {
                return Err(anyhow::anyhow!("Failed to create worktree: {}", stderr));
            }
        }

        // Get the commit hash
        let commit_output = tokio::process::Command::new("git")
            .args(&["rev-parse", "HEAD"])
            .current_dir(&worktree_path)
            .output()
            .await?;

        let commit = String::from_utf8_lossy(&commit_output.stdout).trim().to_string();

        info!("Created worktree {} on branch {}", name, branch_name);

        Ok(WorktreeInfo {
            name: name.to_string(),
            path: worktree_path,
            branch: branch_name,
            commit,
        })
    }

    /// List all worktrees
    pub async fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        let output = tokio::process::Command::new("git")
            .args(&["worktree", "list", "--porcelain"])
            .current_dir(&self.base_dir)
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to list worktrees: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut worktrees = Vec::new();
        let mut current_name = String::new();
        let mut current_path = PathBuf::new();
        let mut current_branch = String::new();
        let mut current_commit = String::new();

        for line in stdout.lines() {
            if line.starts_with("worktree ") {
                if !current_name.is_empty() {
                    worktrees.push(WorktreeInfo {
                        name: current_name.clone(),
                        path: current_path.clone(),
                        branch: current_branch.clone(),
                        commit: current_commit.clone(),
                    });
                }
                current_path = PathBuf::from(line.strip_prefix("worktree ").unwrap_or(""));
                current_name = current_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                current_branch.clear();
                current_commit.clear();
            } else if line.starts_with("branch ") {
                current_branch = line.strip_prefix("branch ").unwrap_or("").to_string();
            } else if line.starts_with("HEAD ") {
                current_commit = line.strip_prefix("HEAD ").unwrap_or("").to_string();
            }
        }

        // Don't forget the last one
        if !current_name.is_empty() {
            worktrees.push(WorktreeInfo {
                name: current_name,
                path: current_path,
                branch: current_branch,
                commit: current_commit,
            });
        }

        Ok(worktrees)
    }

    /// Remove a worktree
    pub async fn remove_worktree(&self, name: &str) -> Result<()> {
        let worktree_path = PathBuf::from(&self.worktrees_dir).join(name);

        info!("Removing worktree: {}", name);

        let output = tokio::process::Command::new("git")
            .args(&["worktree", "remove"])
            .arg(worktree_path.to_str().unwrap())
            .current_dir(&self.base_dir)
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to remove worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        info!("Removed worktree: {}", name);
        Ok(())
    }

    /// Prune stale worktrees
    pub async fn prune_worktrees(&self) -> Result<Vec<String>> {
        let output = tokio::process::Command::new("git")
            .args(&["worktree", "prune"])
            .current_dir(&self.base_dir)
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to prune worktrees: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        debug!("Pruned stale worktrees");
        Ok(vec![])
    }
}
