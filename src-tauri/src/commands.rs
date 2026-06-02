//! Tauri command surface bridging the frontend to the `open_assistant` core.
//!
//! These commands do what the existing `ui::web.rs` / `ui::tui.rs` handlers
//! fail to do: they actually call `Agent::process`. They never reproduce the
//! hardcoded "simulated response" placeholder strings. See openspec change
//! `add-desktop-app`, Phase 2.

use crate::state::AppCore;
use open_assistant::config;
use open_assistant::core::agent::Agent;
use open_assistant::core::session::Session;
use open_assistant::core::Message;
use serde::Serialize;
use tauri::State;

/// Status summary shown in the chat sidebar. Token/cost fields are intentionally
/// omitted (rather than reported as fabricated zeros) since the non-streaming
/// `call_llm` does not yet capture the API `usage` block. See task 2.5.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub model: String,
    pub provider: String,
    pub mode: String,
    pub workspace: String,
    pub message_count: usize,
    pub tools_enabled: bool,
    pub api_key_set: bool,
}

/// Config exposed to the Settings view. The api_key is never sent in clear;
/// only a masked preview plus a boolean indicating whether one is set.
#[derive(Debug, Serialize)]
pub struct ConfigDto {
    pub model: String,
    pub api_base: String,
    pub provider: String,
    pub api_key_masked: String,
    pub api_key_set: bool,
}

fn mask_key(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() {
        return String::new();
    }
    let visible: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("••••••••{visible}")
}

/// Send a user message through the real agent loop and return the assistant reply.
///
/// `Agent::process` appends both the user and assistant `Message`s to the
/// session and returns the assistant text; we wrap that text in a `Message` for
/// the frontend. Errors (including the now-surfaced HTTP/empty-key failures from
/// `call_llm`) are mapped to a `String` so the UI can render an error state.
#[tauri::command]
pub async fn send_message(state: State<'_, AppCore>, message: String) -> Result<Message, String> {
    if message.trim().is_empty() {
        return Err("Cannot send an empty message.".into());
    }
    let mut turn = state.turn.lock().await;
    let crate::state::Turn { agent, ctx, session } = &mut *turn;
    let reply = agent
        .process(&message, ctx, session)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Message::assistant(reply))
}

/// Full transcript for hydrating the message list on load.
#[tauri::command]
pub async fn get_history(state: State<'_, AppCore>) -> Result<Vec<Message>, String> {
    let turn = state.turn.lock().await;
    Ok(turn.session.messages().to_vec())
}

/// Sidebar status. Reloads config so a freshly-saved api_key is reflected.
#[tauri::command]
pub async fn get_status(state: State<'_, AppCore>) -> Result<StatusResponse, String> {
    let turn = state.turn.lock().await;
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    Ok(StatusResponse {
        model: turn.agent.model.clone(),
        provider: cfg.model.provider,
        mode: if turn.agent.tools_enabled { "tools".into() } else { "chat".into() },
        workspace: turn.agent.workspace_dir.clone(),
        message_count: turn.session.messages().len(),
        tools_enabled: turn.agent.tools_enabled,
        api_key_set: !cfg.model.api_key.trim().is_empty(),
    })
}

/// Reset the in-memory transcript. NOTE: this clears only the visible
/// conversation; it does NOT remove the daily-note markdown or reset the
/// learned `UserModel` that `Agent::process` already persisted to the data dir.
#[tauri::command]
pub async fn clear_conversation(state: State<'_, AppCore>) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    turn.session = Session::new("desktop", "local");
    Ok(())
}

/// Read the editable subset of config for the Settings view (api_key masked).
#[tauri::command]
pub async fn get_config() -> Result<ConfigDto, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    Ok(ConfigDto {
        model: cfg.model.model,
        api_base: cfg.model.api_base,
        provider: cfg.model.provider,
        api_key_masked: mask_key(&cfg.model.api_key),
        api_key_set: !cfg.model.api_key.trim().is_empty(),
    })
}

/// Persist settings by mutating the `Config` struct directly + `config::save()`.
///
/// We deliberately bypass `config::set()`, whose allowlist silently no-ops
/// `model.api_base` (and others) via a warn-only arm. `api_key` is only
/// overwritten when a non-empty value is provided, so re-saving from the masked
/// field never wipes an existing key. The in-memory `Agent` is rebuilt if the
/// model changed. See task 2.8.
#[tauri::command]
pub async fn save_config(
    state: State<'_, AppCore>,
    model: String,
    api_base: String,
    api_key: Option<String>,
) -> Result<(), String> {
    let mut cfg = config::load().await.map_err(|e| e.to_string())?;
    cfg.model.model = model.trim().to_string();
    cfg.model.api_base = api_base.trim().to_string();
    if let Some(k) = api_key {
        let k = k.trim();
        if !k.is_empty() {
            cfg.model.api_key = k.to_string();
        }
    }
    config::save(&cfg).await.map_err(|e| e.to_string())?;

    let mut turn = state.turn.lock().await;
    if turn.agent.model != cfg.model.model {
        let tools = turn.agent.tools_enabled;
        let ws = turn.agent.workspace_dir.clone();
        turn.agent = Agent::new(cfg.model.model.clone())
            .with_workspace(ws)
            .with_tools_enabled(tools);
    }
    Ok(())
}

/// Toggle tool execution. Off by default; the Settings UI gates this behind a
/// warning that the model would gain shell/file access on this machine.
#[tauri::command]
pub async fn set_tools_enabled(state: State<'_, AppCore>, enabled: bool) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    turn.agent.tools_enabled = enabled;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_hides_all_but_last_four() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key("   "), "");
        let masked = mask_key("sk-or-v1-ABCD1234");
        assert!(masked.ends_with("1234"));
        assert!(masked.starts_with("••••••••"));
        assert!(!masked.contains("ABCD"));
    }
}
