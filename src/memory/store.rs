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

    pub fn count(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM entries", [], |row| row.get(0)
        )?;
        Ok(count)
    }
}
