// src/gateway/telegram.rs
//! Telegram gateway via the Bot API using long polling (`getUpdates`). No extra
//! crate: plain `reqwest` against `https://api.telegram.org`. Each chat gets its
//! own conversation; the poll loop is single-tasked, so no locking is needed.

use anyhow::Result;
use std::collections::HashMap;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::core::agent::Agent;
use crate::core::persona::{FullContext, Persona};
use crate::core::session::Session;
use crate::gateway::session_store::ChannelSessionStore;

struct Convo {
    ctx: FullContext,
    session: Session,
}

const MAX_SESSION_MESSAGES: usize = 40;

/// Run the Telegram long-poll loop until the process exits.
pub async fn start(config: Config) -> Result<()> {
    let token = config.gateway.telegram_token.clone();
    if token.trim().is_empty() {
        anyhow::bail!("No Telegram token configured (gateway.telegram_token).");
    }
    let data_dir = config.general.data_dir.clone();
    let agent = Agent::new(config.model.model.clone())
        .with_workspace(data_dir.clone())
        .with_tools_enabled(config.tools.enabled)
        .with_permission_mode(crate::core::permissions::PermissionMode::from_str(
            &config.permissions.gateway_mode,
        ));

    let client = reqwest::Client::new();
    let api = format!("https://api.telegram.org/bot{}", token);

    // Confirm the token works and announce who we are.
    match client.get(format!("{}/getMe", api)).send().await {
        Ok(r) => {
            let j: serde_json::Value = r.json().await.unwrap_or_default();
            if j["ok"] == true {
                info!("Telegram connected as @{}", j["result"]["username"].as_str().unwrap_or("?"));
            } else {
                anyhow::bail!("Telegram getMe failed: {}", j["description"].as_str().unwrap_or("invalid token"));
            }
        }
        Err(e) => anyhow::bail!("Telegram getMe request failed: {}", e),
    }

    // Persisted per-chat sessions survive restarts (best-effort: a failed open
    // degrades to in-memory only).
    let store = match ChannelSessionStore::open_default(&data_dir) {
        Ok(s) => Some(s),
        Err(e) => {
            warn!("Telegram: could not open session store ({}); sessions won't persist", e);
            None
        }
    };

    let mut sessions: HashMap<i64, Convo> = HashMap::new();
    let mut offset: i64 = 0;

    loop {
        let url = format!("{}/getUpdates?timeout=30&offset={}", api, offset);
        let updates = match client.get(&url).send().await {
            Ok(r) => match r.json::<serde_json::Value>().await {
                Ok(j) => j,
                Err(e) => {
                    warn!("Telegram: bad getUpdates body: {}", e);
                    continue;
                }
            },
            Err(e) => {
                warn!("Telegram getUpdates error: {} (retrying)", e);
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };

        let Some(results) = updates["result"].as_array() else { continue };
        for update in results {
            if let Some(uid) = update["update_id"].as_i64() {
                offset = uid + 1;
            }
            let message = &update["message"];
            let text = message["text"].as_str().unwrap_or("").trim().to_string();
            let chat_id = message["chat"]["id"].as_i64();
            let (Some(chat_id), false) = (chat_id, text.is_empty()) else { continue };

            // On the first message for this chat after (re)start, restore the
            // persisted session if there is one, else start fresh.
            if !sessions.contains_key(&chat_id) {
                let session = store
                    .as_ref()
                    .and_then(|s| s.load("telegram", &chat_id.to_string()).ok().flatten())
                    .unwrap_or_else(|| Session::new("telegram", chat_id.to_string()));
                let mut ctx = FullContext::new();
                ctx.persona = Persona::load_or_default(&data_dir);
                sessions.insert(chat_id, Convo { ctx, session });
            }
            let convo = sessions.get_mut(&chat_id).expect("just inserted");

            let reply = match agent.process(&text, &mut convo.ctx, &mut convo.session).await {
                Ok(r) => r,
                Err(e) => format!("⚠️ {}", e),
            };

            // Bound session growth (the bot is long-running).
            let len = convo.session.messages.len();
            if len > MAX_SESSION_MESSAGES {
                convo.session.messages.drain(0..(len - MAX_SESSION_MESSAGES));
            }

            // Persist the (bounded) session so it survives a restart.
            if let Some(store) = store.as_ref() {
                if let Err(e) = store.save("telegram", &chat_id.to_string(), &convo.session) {
                    warn!("Telegram: could not persist session for chat {}: {}", chat_id, e);
                }
            }

            if let Err(e) = send_message(&client, &api, chat_id, &reply).await {
                error!("Telegram sendMessage failed: {}", e);
            }
        }
    }
}

/// Telegram caps messages at 4096 chars; split conservatively.
async fn send_message(client: &reqwest::Client, api: &str, chat_id: i64, text: &str) -> Result<()> {
    for chunk in split_chunks(text, 4000) {
        client
            .post(format!("{}/sendMessage", api))
            .json(&serde_json::json!({ "chat_id": chat_id, "text": chunk }))
            .send()
            .await?;
    }
    Ok(())
}

fn split_chunks(s: &str, max: usize) -> Vec<String> {
    if s.trim().is_empty() {
        return vec!["(empty response)".to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if cur.len() + ch.len_utf8() > max {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
