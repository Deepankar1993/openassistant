// src/memory/store.rs
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tracing::{info, debug};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: Option<i64>,
    pub key: String,
    pub value: String,
    pub category: String, // "fact", "preference", "skill", "event", "conversation"
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub importance: f64, // 0.0 to 1.0
}

impl MemoryEntry {
    /// Build a new entry with `created_at`/`updated_at` stamped to now and
    /// `importance` clamped to [0,1]. Lets callers in crates without a `chrono`
    /// dependency (e.g. the desktop app) construct entries.
    pub fn new(
        key: impl Into<String>,
        value: impl Into<String>,
        category: impl Into<String>,
        source: impl Into<String>,
        importance: f64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: None,
            key: key.into(),
            value: value.into(),
            category: category.into(),
            source: source.into(),
            created_at: now,
            updated_at: now,
            importance: importance.clamp(0.0, 1.0),
        }
    }
}

/// A short, human-readable key for a fact: the first few words of its value,
/// lowercased and hyphenated. Shared by the agent's `remember` tool and the
/// desktop fact commands so keys match across both (DRY — divergence would
/// break forget-by-key for panel-added facts).
pub fn fact_key(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "fact".to_string()
    } else {
        slug.chars().take(48).collect()
    }
}

#[derive(Debug)]
pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    pub async fn open_default() -> Result<Self> {
        let data_dir = format!(
            "{}/.openassistant",
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string())
        );
        tokio::fs::create_dir_all(&data_dir).await.ok();
        let db_path = format!("{}/memory.db", data_dir);
        Self::open(&db_path).await
    }

    pub async fn open(db_path: &str) -> Result<Self> {
        info!("Opening memory database: {}", db_path);
        let conn = Connection::open(db_path)?;
        Self::configure(&conn)?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    /// WAL + a busy timeout so the agent, the desktop fact commands, and
    /// status polling — each its own connection — don't hit `SQLITE_BUSY`
    /// when one writes while another reads. Matches the other stores.
    fn configure(conn: &Connection) -> Result<()> {
        conn.busy_timeout(std::time::Duration::from_millis(5_000))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        Ok(())
    }

    /// Synchronous open at `<data_dir>/memory.db`. Used by the agent's sync
    /// prompt builder and the per-call desktop fact commands (matches the
    /// conversation/watcher stores). Honors the configured data dir, unlike
    /// `open_default` which hardcodes `~/.openassistant`.
    pub fn open_in(data_dir: &str) -> Result<Self> {
        std::fs::create_dir_all(data_dir).ok();
        Self::open_sync(&format!("{}/memory.db", data_dir))
    }

    pub fn open_sync(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Self::configure(&conn)?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT 'fact',
                source TEXT NOT NULL DEFAULT 'manual',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                importance REAL NOT NULL DEFAULT 0.5
            );

            CREATE TABLE IF NOT EXISTS sessions_meta (
                id TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                user_id TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                summary TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
                key, value, category,
                content=entries,
                content_rowid=id
            );

            CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
                INSERT INTO entries_fts(rowid, key, value, category)
                VALUES (new.id, new.key, new.value, new.category);
            END;

            CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, key, value, category)
                VALUES ('delete', old.id, old.key, old.value, old.category);
            END;

            CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, key, value, category)
                VALUES ('delete', old.id, old.key, old.value, old.category);
                INSERT INTO entries_fts(rowid, key, value, category)
                VALUES (new.id, new.key, new.value, new.category);
            END;

            CREATE INDEX IF NOT EXISTS idx_entries_category ON entries(category);
            CREATE INDEX IF NOT EXISTS idx_entries_key ON entries(key);
        ")?;
        Ok(())
    }

    pub fn store(&self, entry: &MemoryEntry) -> Result<i64> {
        debug!("Storing memory: {} = {}...", &entry.key[..entry.key.len().min(40)], &entry.value[..entry.value.len().min(40)]);
        self.conn.execute(
            "INSERT INTO entries (key, value, category, source, created_at, updated_at, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &entry.key,
                &entry.value,
                &entry.category,
                &entry.source,
                entry.created_at.to_rfc3339(),
                entry.updated_at.to_rfc3339(),
                entry.importance,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get(&self, key: &str) -> Result<Option<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, key, value, category, source, created_at, updated_at, importance
             FROM entries WHERE key = ?1 ORDER BY updated_at DESC LIMIT 1"
        )?;
        let result = stmt.query_row(params![key], |row| {
            Ok(MemoryEntry {
                id: Some(row.get(0)?),
                key: row.get(1)?,
                value: row.get(2)?,
                category: row.get(3)?,
                source: row.get(4)?,
                created_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
                updated_at: row.get::<_, String>(6)?.parse().unwrap_or_else(|_| Utc::now()),
                importance: row.get(7)?,
            })
        });
        match result {
            Ok(entry) => Ok(Some(entry)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.key, e.value, e.category, e.source, e.created_at, e.updated_at, e.importance
             FROM entries_fts fts
             JOIN entries e ON e.id = fts.rowid
             WHERE entries_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            Ok(MemoryEntry {
                id: Some(row.get(0)?),
                key: row.get(1)?,
                value: row.get(2)?,
                category: row.get(3)?,
                source: row.get(4)?,
                created_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
                updated_at: row.get::<_, String>(6)?.parse().unwrap_or_else(|_| Utc::now()),
                importance: row.get(7)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn list_by_category(&self, category: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, key, value, category, source, created_at, updated_at, importance
             FROM entries WHERE category = ?1 ORDER BY updated_at DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![category, limit as i64], |row| {
            Ok(MemoryEntry {
                id: Some(row.get(0)?),
                key: row.get(1)?,
                value: row.get(2)?,
                category: row.get(3)?,
                source: row.get(4)?,
                created_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
                updated_at: row.get::<_, String>(6)?.parse().unwrap_or_else(|_| Utc::now()),
                importance: row.get(7)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn delete(&self, key: &str) -> Result<usize> {
        let count = self.conn.execute("DELETE FROM entries WHERE key = ?1", params![key])?;
        Ok(count)
    }

    /// All entries, most important / most recent first. Drives the memory
    /// panel and the system-prompt injection.
    pub fn list_all(&self, limit: usize) -> Result<Vec<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, key, value, category, source, created_at, updated_at, importance
             FROM entries ORDER BY importance DESC, updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(MemoryEntry {
                id: Some(row.get(0)?),
                key: row.get(1)?,
                value: row.get(2)?,
                category: row.get(3)?,
                source: row.get(4)?,
                created_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
                updated_at: row.get::<_, String>(6)?.parse().unwrap_or_else(|_| Utc::now()),
                importance: row.get(7)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Edit a fact's value and importance by id (the FTS index is kept in sync
    /// by the `entries_au` trigger).
    pub fn update(&self, id: i64, value: &str, importance: f64) -> Result<usize> {
        let count = self.conn.execute(
            "UPDATE entries SET value = ?1, importance = ?2, updated_at = ?3 WHERE id = ?4",
            params![value, importance.clamp(0.0, 1.0), Utc::now().to_rfc3339(), id],
        )?;
        Ok(count)
    }

    /// Precise one-click forget for the panel (vs. `delete` by key).
    pub fn delete_by_id(&self, id: i64) -> Result<usize> {
        let count = self.conn.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
        Ok(count)
    }

    pub fn count(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM entries", [], |row| row.get(0)
        )?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, value: &str, importance: f64) -> MemoryEntry {
        let now = Utc::now();
        MemoryEntry {
            id: None,
            key: key.into(),
            value: value.into(),
            category: "fact".into(),
            source: "manual".into(),
            created_at: now,
            updated_at: now,
            importance,
        }
    }

    fn store() -> (tempfile::TempDir, MemoryStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = MemoryStore::open_in(dir.path().to_str().unwrap()).unwrap();
        (dir, s)
    }

    #[test]
    fn store_returns_id_and_list_all_orders_by_importance() {
        let (_dir, s) = store();
        let id_low = s.store(&entry("a", "likes tea", 0.3)).unwrap();
        let _id_high = s.store(&entry("b", "works in Rust", 0.9)).unwrap();
        assert!(id_low >= 1);

        let all = s.list_all(10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].value, "works in Rust", "highest importance first");
        assert_eq!(all[1].value, "likes tea");
        assert_eq!(s.count().unwrap(), 2);
    }

    #[test]
    fn update_changes_row_and_keeps_fts_in_sync() {
        let (_dir, s) = store();
        let id = s.store(&entry("proj", "building a CLI", 0.5)).unwrap();
        // Old value is searchable; new is not yet.
        assert_eq!(s.search_fts("CLI", 5).unwrap().len(), 1);
        assert!(s.search_fts("dashboard", 5).unwrap().is_empty());

        s.update(id, "building a dashboard", 0.95).unwrap();

        let row = &s.list_all(10).unwrap()[0];
        assert_eq!(row.value, "building a dashboard");
        assert!((row.importance - 0.95).abs() < 1e-9);
        // FTS reflects the edit (entries_au trigger): new term found, old gone.
        assert_eq!(s.search_fts("dashboard", 5).unwrap().len(), 1);
        assert!(s.search_fts("CLI", 5).unwrap().is_empty());
    }

    #[test]
    fn delete_by_id_removes_one() {
        let (_dir, s) = store();
        let id = s.store(&entry("x", "to forget", 0.5)).unwrap();
        s.store(&entry("y", "to keep", 0.5)).unwrap();
        assert_eq!(s.delete_by_id(id).unwrap(), 1);
        let all = s.list_all(10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].value, "to keep");
    }

    #[test]
    fn importance_is_clamped_on_update() {
        let (_dir, s) = store();
        let id = s.store(&entry("z", "v", 0.5)).unwrap();
        s.update(id, "v", 9.0).unwrap();
        assert!((s.list_all(1).unwrap()[0].importance - 1.0).abs() < 1e-9);
    }
}
