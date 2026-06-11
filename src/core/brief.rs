// src/core/brief.rs
//! Daily Brief — the assistant's proactive morning message.
//!
//! `generate_brief` composes one LLM call from memory, daily notes, the goal
//! board, and recent watcher activity. Delivery scheduling lives in the
//! gateway's proactive loop (`src/gateway/proactive.rs`); the `brief` CLI
//! command prints the same brief on demand.

use anyhow::Result;

use crate::config::Config;
use crate::core::memory::MemoryWorkspace;

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// True when the brief should be sent now: local HH:MM has reached the
/// configured time and no brief went out today. Pure — testable without a
/// clock. Invalid configured times fall back to 08:00.
pub fn brief_due(
    cfg_time: &str,
    last_sent_date: &str,
    now_local: &chrono::DateTime<chrono::Local>,
) -> bool {
    let valid = cfg_time.len() == 5
        && cfg_time.as_bytes()[2] == b':'
        && cfg_time[..2].parse::<u8>().map(|h| h < 24).unwrap_or(false)
        && cfg_time[3..].parse::<u8>().map(|m| m < 60).unwrap_or(false);
    let target = if valid { cfg_time } else { "08:00" };
    let today = now_local.format("%Y-%m-%d").to_string();
    let hhmm = now_local.format("%H:%M").to_string();
    // Zero-padded HH:MM compares correctly as a string.
    hhmm.as_str() >= target && last_sent_date != today
}

/// Compose the daily brief with one LLM call. `watcher_recent` is preformatted
/// text about recent watcher changes (may be empty).
pub async fn generate_brief(cfg: &Config, watcher_recent: &str) -> Result<String> {
    let ws = MemoryWorkspace::from_data_dir(&cfg.general.data_dir);
    let long_term = clip(&ws.read_long_term(), 2000);
    let yesterday = clip(&ws.read_yesterday(), 1500);
    let today = clip(&ws.read_today(), 1500);
    let goals = match crate::core::goal_store::GoalStore::open_default() {
        Ok(store) => clip(&store.format(), 1200),
        Err(_) => String::new(),
    };
    let persona_name = crate::core::persona::Persona::load_or_default(&cfg.general.data_dir).name;

    let prompt = format!(
        "You are {persona_name}, a personal AI assistant writing your user's morning brief.\n\
         Write a warm, concise daily brief (under 200 words). Cover, when present:\n\
         - anything notable from yesterday's notes\n\
         - open goals/tasks worth attention today\n\
         - watcher updates (pages the user asked you to monitor)\n\
         End with one short question inviting a reply. Plain text, no headings.\n\n\
         ## Long-term memory\n{long_term}\n\n\
         ## Yesterday's notes\n{yesterday}\n\n\
         ## Today's notes so far\n{today}\n\n\
         ## Goals\n{goals}\n\n\
         ## Watcher updates (last 24h)\n{w}\n",
        w = if watcher_recent.is_empty() { "(none)" } else { watcher_recent },
    );

    let client = reqwest::Client::new();
    let (api_base, api_key, model) = crate::config::resolve_provider(cfg, "text");
    let messages = vec![serde_json::json!({ "role": "user", "content": prompt })];
    let text =
        crate::core::agent::call_llm_raw(&client, api_base, api_key, model, &messages).await?;
    let text = text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("LLM returned an empty brief");
    }
    let _ = ws.append_daily("Delivered the daily brief.");
    Ok(text)
}

/// Format watcher changes from the last 24h for the brief prompt.
pub fn recent_watcher_summary(store: &crate::core::watchers::WatcherStore) -> String {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
    let mut out = String::new();
    for w in &store.state.watchers {
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&w.last_changed) {
            if t.with_timezone(&chrono::Utc) >= cutoff {
                out.push_str(&format!(
                    "- {} changed at {}{}\n",
                    w.url,
                    w.last_changed,
                    if w.note.is_empty() { String::new() } else { format!(" ({})", w.note) }
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local(h: u32, m: u32) -> chrono::DateTime<chrono::Local> {
        chrono::Local.with_ymd_and_hms(2026, 6, 12, h, m, 0).unwrap()
    }

    #[test]
    fn brief_due_boundaries() {
        let now = local(8, 0);
        // At the configured minute, not yet sent today → due.
        assert!(brief_due("08:00", "", &now));
        assert!(brief_due("08:00", "2026-06-11", &now));
        // Already sent today → not due.
        assert!(!brief_due("08:00", "2026-06-12", &now));
        // Before the configured time → not due.
        assert!(!brief_due("08:01", "", &now));
        // After → due.
        assert!(brief_due("07:30", "", &now));
    }

    #[test]
    fn brief_due_invalid_time_falls_back_to_eight() {
        assert!(!brief_due("nonsense", "", &local(7, 59)));
        assert!(brief_due("nonsense", "", &local(8, 0)));
        assert!(!brief_due("25:99", "", &local(7, 0)));
        assert!(brief_due("25:99", "", &local(9, 0)));
    }
}
