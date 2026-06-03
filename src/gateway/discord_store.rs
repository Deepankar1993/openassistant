// src/gateway/discord_store.rs
//! SQLite persistence for the Discord bot so threads and conversations survive
//! restarts (`~/.openassistant/discord.db`). Owned thread ids are remembered so
//! the bot keeps continuing them; each conversation's `Session` is stored as
//! JSON keyed by channel/thread id.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashSet;

use crate::core::session::Session;

pub struct DiscordStore {
    conn: Connection,
}

impl DiscordStore {
    pub fn open_default() -> Result<Self> {
        let data_dir = crate::config::data_dir_default();
        std::fs::create_dir_all(&data_dir).ok();
        Self::open(&format!("{}/discord.db", data_dir))
    }

    pub fn open(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS owned_threads (
                 thread_id  TEXT PRIMARY KEY,
                 title      TEXT NOT NULL DEFAULT '',
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS conversations (
                 conv_id      TEXT PRIMARY KEY,
                 session_json TEXT NOT NULL,
                 updated_at   TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS claude_sessions (
                 conv_id    TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    /// The Claude Code session id bound to a conversation (for `--resume`).
    pub fn get_claude_session(&self, conv_id: u64) -> Result<Option<String>> {
        let res = self.conn.query_row(
            "SELECT session_id FROM claude_sessions WHERE conv_id = ?1",
            params![conv_id.to_string()],
            |r| r.get::<_, String>(0),
        );
        match res {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_claude_session(&self, conv_id: u64, session_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO claude_sessions (conv_id, session_id, updated_at) VALUES (?1, ?2, ?3)",
            params![conv_id.to_string(), session_id, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// All bot-owned thread ids (continue these without a mention).
    pub fn owned_threads(&self) -> Result<HashSet<u64>> {
        let mut stmt = self.conn.prepare("SELECT thread_id FROM owned_threads")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut set = HashSet::new();
        for r in rows {
            if let Ok(id) = r?.parse::<u64>() {
                set.insert(id);
            }
        }
        Ok(set)
    }

    pub fn mark_thread(&self, thread_id: u64, title: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO owned_threads (thread_id, title, created_at) VALUES (?1, ?2, ?3)",
            params![thread_id.to_string(), title, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn load_session(&self, conv_id: u64) -> Result<Option<Session>> {
        let res = self.conn.query_row(
            "SELECT session_json FROM conversations WHERE conv_id = ?1",
            params![conv_id.to_string()],
            |r| r.get::<_, String>(0),
        );
        match res {
            Ok(json) => Ok(serde_json::from_str(&json).ok()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_session(&self, conv_id: u64, session: &Session) -> Result<()> {
        let json = serde_json::to_string(session)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO conversations (conv_id, session_json, updated_at) VALUES (?1, ?2, ?3)",
            params![conv_id.to_string(), json, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn clear_conversation(&self, conv_id: u64) -> Result<()> {
        self.conn.execute("DELETE FROM conversations WHERE conv_id = ?1", params![conv_id.to_string()])?;
        self.conn.execute("DELETE FROM claude_sessions WHERE conv_id = ?1", params![conv_id.to_string()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_and_threads_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("d.db");
        let path = db.to_str().unwrap();

        {
            let s = DiscordStore::open(path).unwrap();
            s.mark_thread(42, "hello").unwrap();
            let mut sess = Session::new("discord", "42");
            sess.add_message(crate::core::Message::user("hi"));
            s.save_session(42, &sess).unwrap();
        }

        // Reopen — both must persist.
        let s = DiscordStore::open(path).unwrap();
        assert!(s.owned_threads().unwrap().contains(&42));
        let loaded = s.load_session(42).unwrap().unwrap();
        assert_eq!(loaded.messages().len(), 1);

        s.clear_conversation(42).unwrap();
        assert!(s.load_session(42).unwrap().is_none());
    }
}
