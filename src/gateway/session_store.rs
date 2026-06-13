// src/gateway/session_store.rs
//! SQLite persistence for gateway conversations keyed by (channel, external id),
//! so Telegram chats and Slack channels survive restarts the way Discord threads
//! already do (`discord_store.rs`). One table shared across channels; the
//! composite primary key keeps a Telegram chat_id and a Slack channel with the
//! same string from colliding.
//!
//! Lifetime: Telegram holds one connection for the whole single-tasked poll
//! loop; Slack opens one per event handler and drops it after the save (so a
//! connection is never held across an `.await`). Because Slack handlers run
//! concurrently, every connection sets a `busy_timeout` so a second writer
//! waits for the WAL lock instead of failing with `SQLITE_BUSY`.

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::core::session::Session;

pub struct ChannelSessionStore {
    conn: Connection,
}

impl ChannelSessionStore {
    pub fn open_default(data_dir: &str) -> Result<Self> {
        std::fs::create_dir_all(data_dir).ok();
        Self::open(&format!("{}/gateway_sessions.db", data_dir))
    }

    pub fn open(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        // Concurrent Slack handlers each open their own connection; wait for the
        // WAL write lock rather than returning SQLITE_BUSY immediately.
        conn.busy_timeout(std::time::Duration::from_millis(5_000))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS channel_sessions (
                 channel      TEXT NOT NULL,
                 external_id  TEXT NOT NULL,
                 session_json TEXT NOT NULL,
                 updated_at   TEXT NOT NULL,
                 PRIMARY KEY (channel, external_id)
             );",
        )?;
        Ok(Self { conn })
    }

    pub fn load(&self, channel: &str, external_id: &str) -> Result<Option<Session>> {
        let res = self.conn.query_row(
            "SELECT session_json FROM channel_sessions WHERE channel = ?1 AND external_id = ?2",
            params![channel, external_id],
            |r| r.get::<_, String>(0),
        );
        match res {
            Ok(json) => Ok(serde_json::from_str(&json).ok()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, channel: &str, external_id: &str, session: &Session) -> Result<()> {
        let json = serde_json::to_string(session)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO channel_sessions (channel, external_id, session_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![channel, external_id, json, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Message;

    #[test]
    fn save_load_round_trip_and_channel_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.db");
        let store = ChannelSessionStore::open(path.to_str().unwrap()).unwrap();

        // Missing → None.
        assert!(store.load("telegram", "42").unwrap().is_none());

        let mut tg = Session::new("telegram", "42");
        tg.add_message(Message::user("hi from telegram"));
        store.save("telegram", "42", &tg).unwrap();

        // Same external id under a different channel is a distinct row.
        let mut sl = Session::new("slack", "42");
        sl.add_message(Message::user("hi from slack"));
        sl.add_message(Message::assistant("hello"));
        store.save("slack", "42", &sl).unwrap();

        // Reopen from disk — both persist independently.
        let store2 = ChannelSessionStore::open(path.to_str().unwrap()).unwrap();
        let loaded_tg = store2.load("telegram", "42").unwrap().unwrap();
        let loaded_sl = store2.load("slack", "42").unwrap().unwrap();
        assert_eq!(loaded_tg.messages().len(), 1);
        assert_eq!(loaded_sl.messages().len(), 2);
        assert_eq!(loaded_tg.messages()[0].content, "hi from telegram");

        // Overwrite (INSERT OR REPLACE).
        let mut tg2 = Session::new("telegram", "42");
        tg2.add_message(Message::user("a"));
        tg2.add_message(Message::user("b"));
        store2.save("telegram", "42", &tg2).unwrap();
        assert_eq!(store2.load("telegram", "42").unwrap().unwrap().messages().len(), 2);
    }
}
