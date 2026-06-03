// src/core/checkpoint.rs
//! Checkpoint system — save/restore file states (like Claude Code's /rewind).
//!
//! Persisted in a dedicated SQLite database (`~/.openassistant/checkpoints.db`)
//! so checkpoints survive process restarts. We deliberately do NOT reuse
//! `memory.db`: file snapshots are multi-MB UTF-8 blobs that would pressure the
//! FTS page cache, and the memory store does not enable per-connection
//! `foreign_keys` (which we rely on for `ON DELETE CASCADE`).

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

/// A full checkpoint including file bodies. Loaded on demand via
/// [`CheckpointStore::load_checkpoint`]; listing returns [`CheckpointMeta`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub description: String,
    /// Map of file_path -> file_hash (SHA-256)
    pub file_hashes: HashMap<String, String>,
    /// Map of file_path -> file_content (full snapshot)
    pub file_snapshots: HashMap<String, String>,
}

/// Lightweight checkpoint metadata (no file bodies) for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMeta {
    pub id: String,
    pub session_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub description: String,
    pub file_count: usize,
}

/// Result of a restore: which files were written vs skipped (modified since the
/// checkpoint and not force-overwritten).
#[derive(Debug, Clone, Default)]
pub struct RestoreReport {
    pub restored: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Debug)]
pub struct CheckpointStore {
    conn: Connection,
    max_checkpoints: usize,
}

impl CheckpointStore {
    /// Open the default checkpoints database under the data dir.
    pub fn open_default() -> Result<Self> {
        let data_dir = crate::config::data_dir_default();
        std::fs::create_dir_all(&data_dir).ok();
        Self::open(&format!("{}/checkpoints.db", data_dir))
    }

    /// Open (creating if needed) a checkpoints database at `db_path`.
    pub fn open(db_path: &str) -> Result<Self> {
        info!("Opening checkpoint database: {}", db_path);
        let conn = Connection::open(db_path)?;
        // Per-connection pragmas: WAL for concurrent reads, foreign_keys for the
        // ON DELETE CASCADE on checkpoint_files (SQLite defaults this OFF).
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let store = Self { conn, max_checkpoints: 50 };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS checkpoints (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                created_at  TEXT NOT NULL,
                file_count  INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_cp_session ON checkpoints(session_id, created_at);

            CREATE TABLE IF NOT EXISTS checkpoint_files (
                checkpoint_id TEXT NOT NULL REFERENCES checkpoints(id) ON DELETE CASCADE,
                file_path     TEXT NOT NULL,
                file_hash     TEXT NOT NULL,
                content       TEXT NOT NULL,
                PRIMARY KEY (checkpoint_id, file_path)
            );",
        )?;
        Ok(())
    }

    /// Create a checkpoint from the current state of files in a directory.
    /// Binary (non-UTF-8) files are silently skipped (`content` is TEXT).
    pub fn create_checkpoint(
        &mut self,
        session_id: &str,
        description: &str,
        workspace_dir: &str,
        file_paths: &[String],
    ) -> Result<String> {
        let checkpoint_id = format!("cp_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let created_at = chrono::Utc::now().to_rfc3339();

        // Snapshot file contents up front, before touching the DB.
        let mut files: Vec<(String, String, String)> = Vec::new(); // (path, hash, content)
        for path in file_paths {
            let full_path = Path::new(workspace_dir).join(path);
            if full_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
                    files.push((path.clone(), hash, content));
                }
            }
        }

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO checkpoints (id, session_id, description, created_at, file_count)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![checkpoint_id, session_id, description, created_at, files.len() as i64],
        )?;
        for (path, hash, content) in &files {
            tx.execute(
                "INSERT INTO checkpoint_files (checkpoint_id, file_path, file_hash, content)
                 VALUES (?1, ?2, ?3, ?4)",
                params![checkpoint_id, path, hash, content],
            )?;
        }
        tx.commit()?;

        self.prune_old_checkpoints(session_id)?;
        info!(
            "Created checkpoint {} for session {} ({} files)",
            checkpoint_id, session_id, files.len()
        );
        Ok(checkpoint_id)
    }

    /// Restore files to a previous checkpoint state.
    ///
    /// Data-loss guard: a target file whose current content differs from the
    /// snapshot hash is SKIPPED (with a warning) unless `force` is set, so we
    /// never silently clobber edits made since the checkpoint.
    pub fn restore_checkpoint(
        &self,
        checkpoint_id: &str,
        workspace_dir: &str,
        force: bool,
    ) -> Result<RestoreReport> {
        if !self.checkpoint_exists(checkpoint_id)? {
            anyhow::bail!("Checkpoint not found: {}", checkpoint_id);
        }
        let files = self.load_checkpoint_files(checkpoint_id)?;

        let mut report = RestoreReport::default();
        for (path, snap_hash, content) in files {
            let full_path = Path::new(workspace_dir).join(&path);

            if !force && full_path.exists() {
                if let Ok(current) = std::fs::read_to_string(&full_path) {
                    let cur_hash = format!("{:x}", sha2::Sha256::digest(current.as_bytes()));
                    if cur_hash != snap_hash {
                        warn!("Skipping {} — modified since checkpoint (use force to overwrite)", path);
                        report.skipped.push(path);
                        continue;
                    }
                }
            }

            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full_path, &content)?;
            report.restored.push(path);
        }

        info!(
            "Restored {} files ({} skipped) from checkpoint {}",
            report.restored.len(), report.skipped.len(), checkpoint_id
        );
        Ok(report)
    }

    /// Load a full checkpoint (with file bodies) by id.
    pub fn load_checkpoint(&self, checkpoint_id: &str) -> Result<Option<Checkpoint>> {
        let meta = self.conn.query_row(
            "SELECT id, session_id, description, created_at FROM checkpoints WHERE id = ?1",
            params![checkpoint_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        );
        let (id, session_id, description, created_at) = match meta {
            Ok(t) => t,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let mut file_hashes = HashMap::new();
        let mut file_snapshots = HashMap::new();
        for (path, hash, content) in self.load_checkpoint_files(checkpoint_id)? {
            file_hashes.insert(path.clone(), hash);
            file_snapshots.insert(path, content);
        }

        Ok(Some(Checkpoint {
            id,
            session_id,
            timestamp: created_at.parse().unwrap_or_else(|_| chrono::Utc::now()),
            description,
            file_hashes,
            file_snapshots,
        }))
    }

    /// List checkpoint metadata for a session (no file bodies).
    pub fn list_checkpoints(&self, session_id: &str) -> Result<Vec<CheckpointMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, description, created_at, file_count
             FROM checkpoints WHERE session_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(CheckpointMeta {
                id: row.get(0)?,
                session_id: row.get(1)?,
                description: row.get(2)?,
                timestamp: row.get::<_, String>(3)?.parse().unwrap_or_else(|_| chrono::Utc::now()),
                file_count: row.get::<_, i64>(4)? as usize,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// The most recent checkpoint for a session.
    pub fn latest_checkpoint(&self, session_id: &str) -> Result<Option<CheckpointMeta>> {
        Ok(self.list_checkpoints(session_id)?.into_iter().last())
    }

    /// Format the checkpoint list for display.
    pub fn format_checkpoints(&self, session_id: &str) -> String {
        let checkpoints = match self.list_checkpoints(session_id) {
            Ok(c) => c,
            Err(e) => return format!("Error listing checkpoints: {}", e),
        };
        if checkpoints.is_empty() {
            return "No checkpoints for this session.".to_string();
        }

        let mut output = format!("📸 Checkpoints ({}):\n", checkpoints.len());
        output.push_str(&"─".repeat(50));
        output.push('\n');
        for cp in &checkpoints {
            output.push_str(&format!(
                "  [{}] {} — {} files — {}\n",
                cp.id,
                cp.timestamp.format("%Y-%m-%d %H:%M:%S"),
                cp.file_count,
                cp.description
            ));
        }
        output
    }

    fn checkpoint_exists(&self, id: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM checkpoints WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    fn load_checkpoint_files(&self, checkpoint_id: &str) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_path, file_hash, content FROM checkpoint_files
             WHERE checkpoint_id = ?1 ORDER BY file_path",
        )?;
        let rows = stmt.query_map(params![checkpoint_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Keep only the most recent `max_checkpoints` for a session; older ones
    /// (and their files, via cascade) are deleted.
    fn prune_old_checkpoints(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM checkpoints WHERE id IN (
                 SELECT id FROM checkpoints WHERE session_id = ?1
                 ORDER BY created_at DESC LIMIT -1 OFFSET ?2
             )",
            params![session_id, self.max_checkpoints as i64],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (CheckpointStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("cp.db");
        let store = CheckpointStore::open(db.to_str().unwrap()).unwrap();
        (store, dir)
    }

    #[test]
    fn test_create_and_restore_checkpoint() {
        let (mut store, dir) = temp_store();
        let workspace = dir.path().to_str().unwrap();
        let test_file = "test.txt";
        std::fs::write(format!("{}/{}", workspace, test_file), "original content").unwrap();

        let cp_id = store
            .create_checkpoint("session_1", "Initial state", workspace, &[test_file.to_string()])
            .unwrap();

        // Modify the file after the checkpoint.
        std::fs::write(format!("{}/{}", workspace, test_file), "modified content").unwrap();

        // Without force, a modified file is skipped (data-loss guard).
        let report = store.restore_checkpoint(&cp_id, workspace, false).unwrap();
        assert_eq!(report.restored.len(), 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(
            std::fs::read_to_string(format!("{}/{}", workspace, test_file)).unwrap(),
            "modified content"
        );

        // With force, it is overwritten back to the snapshot.
        let report = store.restore_checkpoint(&cp_id, workspace, true).unwrap();
        assert_eq!(report.restored.len(), 1);
        assert_eq!(
            std::fs::read_to_string(format!("{}/{}", workspace, test_file)).unwrap(),
            "original content"
        );
    }

    #[test]
    fn test_persistence_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cp.db").to_str().unwrap().to_string();
        let workspace = dir.path().to_str().unwrap();
        std::fs::write(format!("{}/a.txt", workspace), "a").unwrap();

        let id = {
            let mut store = CheckpointStore::open(&db_path).unwrap();
            store
                .create_checkpoint("s1", "First", workspace, &["a.txt".to_string()])
                .unwrap()
        };

        // Reopen the DB in a fresh store — the checkpoint must persist.
        let store = CheckpointStore::open(&db_path).unwrap();
        let list = store.list_checkpoints("s1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].file_count, 1);
    }

    #[test]
    fn test_list_checkpoints() {
        let (mut store, dir) = temp_store();
        let workspace = dir.path().to_str().unwrap();
        std::fs::write(format!("{}/a.txt", workspace), "a").unwrap();

        store.create_checkpoint("s1", "First", workspace, &["a.txt".to_string()]).unwrap();
        store.create_checkpoint("s1", "Second", workspace, &["a.txt".to_string()]).unwrap();
        store.create_checkpoint("s2", "Other session", workspace, &["a.txt".to_string()]).unwrap();

        assert_eq!(store.list_checkpoints("s1").unwrap().len(), 2);
        assert_eq!(store.list_checkpoints("s2").unwrap().len(), 1);
    }
}
