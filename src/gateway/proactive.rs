// src/gateway/proactive.rs
//! The proactive loop — the assistant messages you first.
//!
//! Spawned by `run_all`: a 60s tick that (a) delivers the daily brief at the
//! configured local time and (b) checks due URL watchers, posting change
//! notifications. Config is re-read every tick (live enable/disable, the
//! `review_loop` pattern); every step logs-and-continues so the gateway
//! never dies from a proactive failure.

use serenity::http::Http;
use serenity::model::id::ChannelId;
use tracing::{info, warn};

use crate::config::Config;
use crate::core::brief;
use crate::core::watchers::WatcherStore;

pub async fn proactive_loop(initial: Config) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    info!("Proactive loop running (daily brief + watchers).");
    loop {
        tick.tick().await;
        // Live config: edits to [brief] or watchers apply without a restart.
        let cfg = match crate::config::load().await {
            Ok(c) => c,
            Err(_) => initial.clone(),
        };

        run_brief_step(&cfg).await;
        run_watcher_step(&cfg).await;
    }
}

async fn run_brief_step(cfg: &Config) {
    if !cfg.brief.enabled {
        return;
    }
    let store = WatcherStore::open(&cfg.general.data_dir);
    let now_local = chrono::Local::now();
    if !brief::brief_due(&cfg.brief.time, &store.state.last_brief_date, &now_local) {
        return;
    }
    let watcher_recent = brief::recent_watcher_summary(&store);
    drop(store);

    match brief::generate_brief(cfg, &watcher_recent).await {
        Ok(text) => {
            post_everywhere(cfg, &format!("☀️ Daily brief\n\n{}", text)).await;
            // Mark delivered only after a successful generation, so transient
            // LLM failures retry on the next tick instead of skipping a day.
            // Caveat: two gateway instances against one data dir could each
            // deliver once — but dual instances already double-answer every
            // channel message, so that setup is unsupported anyway.
            let mut store = WatcherStore::open(&cfg.general.data_dir);
            store.state.last_brief_date = now_local.format("%Y-%m-%d").to_string();
            if let Err(e) = store.save() {
                warn!("could not persist last_brief_date: {}", e);
            }
            info!("Daily brief delivered.");
        }
        Err(e) => warn!("daily brief generation failed (will retry next tick): {}", e),
    }
}

async fn run_watcher_step(cfg: &Config) {
    let mut store = WatcherStore::open(&cfg.general.data_dir);
    if store.state.watchers.is_empty() {
        return;
    }
    let changes = match store.check_due(chrono::Utc::now()).await {
        Ok(c) => c,
        Err(e) => {
            warn!("watcher check failed: {}", e);
            return;
        }
    };
    for change in changes {
        let summary = summarize_change(cfg, &change.body).await;
        let note = if change.note.is_empty() {
            String::new()
        } else {
            format!(" ({})", change.note)
        };
        let text = match summary {
            Some(s) => format!("🔭 {}{} changed:\n{}", change.url, note, s),
            None => format!("🔭 {}{} changed.", change.url, note),
        };
        post_everywhere(cfg, &text).await;
    }
}

/// One-line LLM summary of the new page content; None on any failure (the
/// plain notification still goes out).
async fn summarize_change(cfg: &Config, body: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let (api_base, api_key, model) = crate::config::resolve_provider(cfg, "text");
    let prompt = format!(
        "A web page the user watches just changed. In 1-2 plain sentences, say what the page currently shows. Page text (truncated):\n{}",
        body
    );
    let messages = vec![serde_json::json!({ "role": "user", "content": prompt })];
    match crate::core::agent::call_llm_raw(&client, api_base, api_key, model, &messages).await {
        Ok(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// Post to every configured proactive channel (Discord home channel and/or a
/// Telegram chat). Logs and continues on per-channel failure.
async fn post_everywhere(cfg: &Config, text: &str) {
    let mut delivered = false;

    if cfg.brief.discord && !cfg.gateway.discord_token.is_empty() {
        if let Ok(channel) = cfg.gateway.discord_home_channel.trim().parse::<u64>() {
            let http = Http::new(&cfg.gateway.discord_token);
            for chunk in chunk_text(text, 1900) {
                if let Err(e) = ChannelId::new(channel).say(&http, chunk).await {
                    warn!("proactive Discord post failed: {}", e);
                    break;
                }
                delivered = true;
            }
        }
    }

    if !cfg.brief.telegram_chat_id.trim().is_empty() && !cfg.gateway.telegram_token.is_empty() {
        if let Ok(chat_id) = cfg.brief.telegram_chat_id.trim().parse::<i64>() {
            let client = reqwest::Client::new();
            let api = format!("https://api.telegram.org/bot{}", cfg.gateway.telegram_token);
            for chunk in chunk_text(text, 4000) {
                let res = client
                    .post(format!("{}/sendMessage", api))
                    .json(&serde_json::json!({ "chat_id": chat_id, "text": chunk }))
                    .send()
                    .await;
                match res {
                    Ok(r) if r.status().is_success() => delivered = true,
                    Ok(r) => {
                        warn!("proactive Telegram post failed: HTTP {}", r.status());
                        break;
                    }
                    Err(e) => {
                        warn!("proactive Telegram post failed: {}", e);
                        break;
                    }
                }
            }
        }
    }

    if !delivered {
        info!("Proactive message had no configured channel; skipped: {}", first_line(text));
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// Split on char boundaries into chunks of at most `max` characters.
fn chunk_text(s: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut count = 0usize;
    for ch in s.chars() {
        if count >= max {
            out.push(std::mem::take(&mut cur));
            count = 0;
        }
        cur.push(ch);
        count += 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_text_splits_on_char_boundaries() {
        let chunks = chunk_text(&"é".repeat(10), 4);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chars().count(), 4);
        assert_eq!(chunks[2].chars().count(), 2);
        assert!(chunk_text("", 4).is_empty());
    }
}
