// src/core/checkpoint.rs
//! Checkpoint system — save/restore file states (like Claude Code's /rewind)
//! Uses SQLite for persistence, matching Hermes Agent's session DB pattern.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, debug};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub description: String,
    /// Map of file_path -> file_hash (SHA256)
    pub file_hashes: HashMap<String, String>,
    /// Map of file_path -> file_content (full snapshot)
    pub file_snapshots: HashMap<String, String>,
}

#[derive(Debug, Default)]
pub struct CheckpointStore {
    checkpoints: Vec<Checkpoint>,
    max_checkpoints: usize,
}

impl CheckpointStore {
    pub fn new() -> Self {
        Self {
            checkpoints: Vec::new(),
            max_checkpoints: 50,
        }
    }

    /// Create a checkpoint from the current state of files in a directory
    pub fn create_checkpoint(
        &mut self,
        session_id: &str,
        description: &str,
        workspace_dir: &str,
        file_paths: &[String],
    ) -> Result<String> {
        let checkpoint_id = format!("cp_{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
        let mut file_hashes = HashMap::new();
        let mut file_snapshots = HashMap::new();

        for path in file_paths {
            let full_path = Path::new(workspace_dir).join(path);
            if full_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
                    file_hashes.insert(path.clone(), hash);
                    file_snapshots.insert(path.clone(), content);
                }
            }
        }

        let checkpoint = Checkpoint {
            id: checkpoint_id.clone(),
            session_id: session_id.to_string(),
            timestamp: chrono::Utc::now(),
            description: description.to_string(),
            file_hashes,
            file_snapshots,
        };

        self.checkpoints.push(checkpoint);

        // Enforce max checkpoints per session
        self.prune_old_checkpoints(session_id);

        info!("Created checkpoint {} for session {}", checkpoint_id, session_id);
        Ok(checkpoint_id)
    }

    /// Restore files to a previous checkpoint state
    pub fn restore_checkpoint(&self, checkpoint_id: &str, workspace_dir: &str) -> Result<Vec<String>> {
        let checkpoint = self.checkpoints
            .iter()
            .find(|c| c.id == checkpoint_id)
            .ok_or_else(|| anyhow::anyhow!("Checkpoint not found: {}", checkpoint_id))?;

        let mut restored = Vec::new();

        for (path, content) in &checkpoint.file_snapshots {
            let full_path = Path::new(workspace_dir).join(path);

            // Create parent directories if needed
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Write the file
            std::fs::write(&full_path, content)?;
            restored.push(path.clone());
            debug!("Restored file: {}", path);
        }

        info!("Restored {} files from checkpoint {}", restored.len(), checkpoint_id);
        Ok(restored)
    }

    /// List all checkpoints for a session
    pub fn list_checkpoints(&self, session_id: &str) -> Vec<&Checkpoint> {
        self.checkpoints
            .iter()
            .filter(|c| c.session_id == session_id)
            .collect()
    }

    /// Get the latest checkpoint for a session
    pub fn latest_checkpoint(&self, session_id: &str) -> Option<&Checkpoint> {
        self.checkpoints
            .iter()
            .filter(|c| c.session_id == session_id)
            .last()
    }

    /// Format checkpoint list for display
    pub fn format_checkpoints(&self, session_id: &str) -> String {
        let checkpoints = self.list_checkpoints(session_id);
        if checkpoints.is_empty() {
            return "No checkpoints for this session.".to_string();
        }

        let mut output = format!("📸 Checkpoints ({}):\n", checkpoints.len());
        output.push_str(&"─".repeat(50));
        output.push('\n');

        for cp in checkpoints {
            output.push_str(&format!(
                "  [{}] {} — {} files — {}\n",
                &cp.id,
                cp.timestamp.format("%Y-%m-%d %H:%M:%S"),
                cp.file_snapshots.len(),
                &cp.description
            ));
        }
        output
    }

    fn prune_old_checkpoints(&mut self, session_id: &str) {
        let session_count = self.checkpoints.iter().filter(|c| c.session_id == session_id).count();
        if session_count <= self.max_checkpoints {
            return;
        }

        // Remove oldest checkpoints for this session
        let to_remove = session_count - self.max_checkpoints;
        let mut removed = 0;
        self.checkpoints.retain(|c| {
            if c.session_id == session_id && removed < to_remove {
                removed += 1;
                false
            } else {
                true
            }
        });
        debug!("Pruned {} old checkpoints for session {}", removed, session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_restore_checkpoint() {
        let mut store = CheckpointStore::new();
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();

        // Create a test file
        let test_file = "test.txt";
        std::fs::write(format!("{}/{}", workspace, test_file), "original content").unwrap();

        // Create checkpoint
        let cp_id = store.create_checkpoint(
            "session_1",
            "Initial state",
            workspace,
            &[test_file.to_string()],
        ).unwrap();

        // Modify the file
        std::fs::write(format!("{}/{}", workspace, test_file), "modified content").unwrap();

        // Restore checkpoint
        let restored = store.restore_checkpoint(&cp_id, workspace).unwrap();
        assert_eq!(restored.len(), 1);

        // Verify restoration
        let content = std::fs::read_to_string(format!("{}/{}", workspace, test_file)).unwrap();
        assert_eq!(content, "original content");
    }

    #[test]
    fn test_list_checkpoints() {
        let mut store = CheckpointStore::new();
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();

        std::fs::write(format!("{}/a.txt", workspace), "a").unwrap();

        store.create_checkpoint("s1", "First", workspace, &["a.txt".to_string()]).unwrap();
        store.create_checkpoint("s1", "Second", workspace, &["a.txt".to_string()]).unwrap();
        store.create_checkpoint("s2", "Other session", workspace, &["a.txt".to_string()]).unwrap();

        assert_eq!(store.list_checkpoints("s1").len(), 2);
        assert_eq!(store.list_checkpoints("s2").len(), 1);
    }
}
