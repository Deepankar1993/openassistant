// src/core/conversation_store.rs
//! SQLite persistence for chat conversations so the WebChat and desktop app
//! keep a switchable history across restarts (`<data_dir>/conversations.db`).
//!
//! A conversation's id IS its `Session.id` (a UUID), so there is no parallel
//! id scheme. Follows the `discord_store` pattern: WAL mode, JSON-serialized
//! `Session`, opened per-operation (cheap for a local single-user app, and it
//! avoids holding a `Connection` across an `.await`).

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::core::session::Session;

/// Lightweight row for the sidebar list — no message bodies.
#[derive(Debug, Clone, Serialize)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub message_count: usize,
}

pub struct ConversationStore {
    conn: Connection,
}

impl ConversationStore {
    pub fn open_default(data_dir: &str) -> Result<Self> {
        std::fs::create_dir_all(data_dir).ok();
        Self::open(&format!("{}/conversations.db", data_dir))
    }

    pub fn open(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS conversations (
                 id            TEXT PRIMARY KEY,
                 title         TEXT NOT NULL DEFAULT '',
                 session_json  TEXT NOT NULL,
                 message_count INTEGER NOT NULL DEFAULT 0,
                 created_at    TEXT NOT NULL,
                 updated_at    TEXT NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    /// Persist a session (INSERT OR REPLACE, keyed by `session.id`). When
    /// `title` is None and no title is stored yet, one is derived from the
    /// first user message; an already-stored non-empty title is preserved.
    pub fn save(&self, session: &Session, title: Option<&str>) -> Result<()> {
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT title FROM conversations WHERE id = ?1",
                params![session.id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .filter(|t| !t.is_empty());

        let title = match title {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => existing.unwrap_or_else(|| derive_title(session)),
        };

        let json = serde_json::to_string(session)?;
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO conversations (id, title, session_json, message_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 title = excluded.title,
                 session_json = excluded.session_json,
                 message_count = excluded.message_count,
                 updated_at = excluded.updated_at",
            params![
                session.id,
                title,
                json,
                session.messages().len() as i64,
                now,
            ],
        )?;
        Ok(())
    }

    /// Conversation rows for the sidebar, newest-updated first.
    pub fn list_meta(&self) -> Result<Vec<ConversationMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, updated_at, message_count
             FROM conversations ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ConversationMeta {
                id: r.get(0)?,
                title: r.get(1)?,
                updated_at: r.get(2)?,
                message_count: r.get::<_, i64>(3)?.max(0) as usize,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn load(&self, id: &str) -> Result<Option<Session>> {
        let res = self.conn.query_row(
            "SELECT session_json FROM conversations WHERE id = ?1",
            params![id],
            |r| r.get::<_, String>(0),
        );
        match res {
            Ok(json) => Ok(serde_json::from_str(&json).ok()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM conversations WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }
}

/// A short title from the first user message (≤ 48 chars). Empty conversation
/// ⇒ "New conversation".
pub fn derive_title(session: &Session) -> String {
    let first_user = session.messages().iter().find(|m| m.role == "user");
    match first_user {
        Some(m) => {
            let line = m.content.lines().next().unwrap_or("").trim();
            if line.is_empty() {
                return "New conversation".to_string();
            }
            let truncated: String = line.chars().take(48).collect();
            if line.chars().count() > 48 {
                format!("{}…", truncated.trim_end())
            } else {
                truncated
            }
        }
        None => "New conversation".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Message;

    fn store() -> (tempfile::TempDir, ConversationStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = ConversationStore::open(dir.path().join("c.db").to_str().unwrap()).unwrap();
        (dir, s)
    }

    #[test]
    fn save_load_list_delete_round_trip() {
        let (_dir, store) = store();
        let mut sess = Session::new("desktop", "local");
        sess.add_message(Message::user("Help me plan a trip to Japan"));
        sess.add_message(Message::assistant("Sure!"));
        store.save(&sess, None).unwrap();

        let metas = store.list_meta().unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].id, sess.id);
        assert_eq!(metas[0].message_count, 2);
        assert_eq!(metas[0].title, "Help me plan a trip to Japan");

        let loaded = store.load(&sess.id).unwrap().unwrap();
        assert_eq!(loaded.messages().len(), 2);

        assert!(store.delete(&sess.id).unwrap());
        assert!(store.load(&sess.id).unwrap().is_none());
        assert!(store.list_meta().unwrap().is_empty());
        assert!(!store.delete(&sess.id).unwrap()); // already gone
    }

    #[test]
    fn list_is_ordered_newest_updated_first() {
        let (_dir, store) = store();
        let mut a = Session::new("desktop", "local");
        a.add_message(Message::user("first"));
        store.save(&a, None).unwrap();
        let mut b = Session::new("desktop", "local");
        b.add_message(Message::user("second"));
        store.save(&b, None).unwrap();
        // Touch `a` again so it becomes most-recent.
        a.add_message(Message::user("first again"));
        store.save(&a, None).unwrap();

        let metas = store.list_meta().unwrap();
        assert_eq!(metas[0].id, a.id, "most recently saved is first");
    }

    #[test]
    fn title_is_derived_once_and_preserved() {
        let (_dir, store) = store();
        let mut sess = Session::new("desktop", "local");
        sess.add_message(Message::user("Original question"));
        store.save(&sess, None).unwrap();
        // A later save with no explicit title must not change the title.
        sess.add_message(Message::assistant("answer"));
        sess.add_message(Message::user("a different follow-up"));
        store.save(&sess, None).unwrap();
        assert_eq!(store.list_meta().unwrap()[0].title, "Original question");
    }

    #[test]
    fn derive_title_handles_truncation_and_empty() {
        let empty = Session::new("desktop", "local");
        assert_eq!(derive_title(&empty), "New conversation");

        let mut long = Session::new("desktop", "local");
        long.add_message(Message::user(
            "This is a really long opening message that should be truncated at forty-eight characters for the sidebar",
        ));
        let t = derive_title(&long);
        assert!(t.ends_with('…'));
        assert!(t.chars().count() <= 49); // 48 + ellipsis
    }
}
