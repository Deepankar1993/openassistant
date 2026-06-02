//! Chat + status commands.

use crate::state::AppCore;
use open_assistant::config;
use open_assistant::core::memory::MemoryWorkspace;
use open_assistant::core::session::Session;
use open_assistant::core::Message;
use open_assistant::memory::store::MemoryStore;
use serde::Serialize;
use tauri::State;

/// Status summary shown in the chat sidebar and the Status panel. Token/cost
/// fields are intentionally omitted (rather than reported as fabricated zeros)
/// since the non-streaming `call_llm` does not capture the API `usage` block.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub model: String,
    pub provider: String,
    pub mode: String,
    pub workspace: String,
    pub data_dir: String,
    pub message_count: usize,
    pub tools_enabled: bool,
    pub api_key_set: bool,
    pub memory_db_entries: i64,
    pub memory_md_chars: usize,
}

/// Send a user message through the real agent loop and return the assistant reply.
///
/// `Agent::process` appends both the user and assistant `Message`s to the
/// session and returns the assistant text; we wrap that text in a `Message` for
/// the frontend. Errors (including the surfaced HTTP/empty-key failures from
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

/// Sidebar/Status panel snapshot. All disk I/O happens before locking so a slow
/// read never holds the turn mutex and stalls an in-flight `send_message`.
#[tauri::command]
pub async fn get_status(state: State<'_, AppCore>) -> Result<StatusResponse, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    // Best-effort memory metrics: a missing/locked DB must not fail the status call.
    let memory_db_entries = match MemoryStore::open_default().await {
        Ok(store) => store.count().unwrap_or(0),
        Err(_) => 0,
    };
    let memory_md_chars = MemoryWorkspace::from_data_dir(&cfg.general.data_dir)
        .read_long_term()
        .len();

    let turn = state.turn.lock().await;
    Ok(StatusResponse {
        model: turn.agent.model.clone(),
        provider: cfg.model.provider,
        mode: if turn.agent.tools_enabled { "tools".into() } else { "chat".into() },
        workspace: turn.agent.workspace_dir.clone(),
        data_dir: cfg.general.data_dir,
        message_count: turn.session.messages().len(),
        tools_enabled: turn.agent.tools_enabled,
        api_key_set: !cfg.model.api_key.trim().is_empty(),
        memory_db_entries,
        memory_md_chars,
    })
}

/// Reset the in-memory transcript. NOTE: clears only the visible conversation;
/// it does NOT remove the daily-note markdown or reset the learned `UserModel`
/// that `Agent::process` already persisted to the data dir.
#[tauri::command]
pub async fn clear_conversation(state: State<'_, AppCore>) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    turn.session = Session::new("desktop", "local");
    Ok(())
}
