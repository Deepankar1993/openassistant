// src/core/goal_store.rs
//! Persistence for the goal/subgoal/task board.
//!
//! MVP storage is a single JSON document at `~/.openassistant/goals.json`,
//! written **atomically** (temp file in the same directory + rename) so a crash
//! mid-write cannot truncate it. Single-writer is assumed; concurrent CLI
//! invocations are last-writer-wins. A SQLite backend is the planned next step.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::info;

use super::goal_system::TaskBoard;

pub struct GoalStore {
    path: PathBuf,
    pub board: TaskBoard,
}

impl GoalStore {
    /// Open the default goal store under the data dir.
    pub fn open_default() -> Result<Self> {
        let data_dir = crate::config::data_dir_default();
        std::fs::create_dir_all(&data_dir).ok();
        Self::open(PathBuf::from(format!("{}/goals.json", data_dir)))
    }

    /// Open (or initialize) a goal store at `path`.
    pub fn open(path: PathBuf) -> Result<Self> {
        let board = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            serde_json::from_str(&text).unwrap_or_default()
        } else {
            TaskBoard::new()
        };
        Ok(Self { path, board })
    }

    /// Atomically persist the board.
    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.board)?;
        let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).ok();
        let mut tmp = tempfile::NamedTempFile::new_in(dir)
            .with_context(|| format!("creating temp file in {}", dir.display()))?;
        tmp.write_all(json.as_bytes())?;
        tmp.flush()?;
        // persist() does an atomic rename within the same directory.
        tmp.persist(&self.path)
            .map_err(|e| anyhow::anyhow!("persisting {}: {}", self.path.display(), e))?;
        Ok(())
    }

    // ── Mutating delegates (persist after each change) ──────────────────────

    pub fn create_goal(&mut self, title: &str, description: &str) -> Result<String> {
        let id = self.board.create_goal(title, description);
        self.save()?;
        info!("Created goal {}", id);
        Ok(id)
    }

    pub fn add_subgoal(&mut self, goal_id: &str, title: &str, description: &str) -> Result<Option<String>> {
        let id = self.board.create_subgoal(goal_id, title, description);
        if id.is_some() {
            self.save()?;
        }
        Ok(id)
    }

    pub fn complete_subgoal(&mut self, goal_id: &str, subgoal_id: &str) -> Result<bool> {
        let ok = self.board.complete_subgoal(goal_id, subgoal_id);
        if ok {
            self.save()?;
        }
        Ok(ok)
    }

    pub fn add_task(&mut self, goal_id: &str, subgoal_id: &str, title: &str) -> Result<Option<String>> {
        let id = self.board.add_task_to_subgoal(goal_id, subgoal_id, title);
        if id.is_some() {
            self.save()?;
        }
        Ok(id)
    }

    /// Resolve a goal id from an exact-or-prefix id match, or a title match —
    /// convenient for CLI/tool callers who don't have the full UUID.
    pub fn resolve_goal_id(&self, needle: &str) -> Option<String> {
        self.board.list_goals().into_iter().find_map(|g| {
            if g.id == needle || g.id.starts_with(needle) || g.title.eq_ignore_ascii_case(needle) {
                Some(g.id.clone())
            } else {
                None
            }
        })
    }

    pub fn format(&self) -> String {
        let mut out = self.board.format_board();
        // Append subgoal/id detail so callers can reference subgoals.
        for goal in self.board.list_goals() {
            if goal.subgoals.is_empty() {
                continue;
            }
            out.push_str(&format!("\n🎯 {} [{}]\n", goal.title, &goal.id[..goal.id.len().min(8)]));
            for sg in &goal.subgoals {
                let mark = if sg.completed { "✅" } else { "⬜" };
                out.push_str(&format!(
                    "   {} {} [{}] ({} task(s))\n",
                    mark,
                    sg.title,
                    &sg.id[..sg.id.len().min(8)],
                    sg.tasks.len()
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goals_persist_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("goals.json");

        let goal_id = {
            let mut store = GoalStore::open(path.clone()).unwrap();
            let gid = store.create_goal("Ship v1", "first release").unwrap();
            store.add_subgoal(&gid, "Wire update", "").unwrap();
            gid
        };

        // Reopen — the goal + subgoal must survive.
        let store = GoalStore::open(path).unwrap();
        let goals = store.board.list_goals();
        assert_eq!(goals.len(), 1);
        let g = store.board.get_goal(&goal_id).unwrap();
        assert_eq!(g.subgoals.len(), 1);
        assert_eq!(g.progress(), (0, 1));
    }
}
