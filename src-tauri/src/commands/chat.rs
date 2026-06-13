//! Chat + status commands.

use crate::state::AppCore;
use open_assistant::config;
use open_assistant::core::conversation_store::{ConversationMeta, ConversationStore};
use open_assistant::core::memory::MemoryWorkspace;
use open_assistant::core::session::Session;
use open_assistant::core::Message;
use open_assistant::memory::store::MemoryStore;
use serde::Serialize;
use tauri::State;

/// Persist the turn's session to the conversation store (best-effort: a failed
/// save logs and never breaks the turn). Empty sessions are not written, so the
/// history sidebar has no blank entries.
fn persist_session(turn: &crate::state::Turn) {
    if turn.session.messages().is_empty() {
        return;
    }
    match ConversationStore::open_default(&turn.agent.workspace_dir) {
        Ok(store) => {
            if let Err(e) = store.save(&turn.session, None) {
                log::warn!("could not persist conversation: {}", e);
            }
        }
        Err(e) => log::warn!("could not open conversation store: {}", e),
    }
}

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
#[tauri::command(rename_all = "snake_case")]
pub async fn send_message(state: State<'_, AppCore>, message: String) -> Result<Message, String> {
    if message.trim().is_empty() {
        return Err("Cannot send an empty message.".into());
    }
    let mut turn = state.turn.lock().await;
    let reply = {
        let crate::state::Turn { agent, ctx, session } = &mut *turn;
        agent
            .process(&message, ctx, session)
            .await
            .map_err(|e| e.to_string())?
    };
    persist_session(&turn);
    Ok(Message::assistant(reply))
}

/// Streaming variant of `send_message`: forwards every `AgentEvent` (tokens,
/// tool steps, done/error) to the window as a `chat-event` Tauri event while
/// the turn runs, and still resolves with the final assistant `Message` so
/// callers without event support (mock mode) keep working.
#[tauri::command(rename_all = "snake_case")]
pub async fn send_message_stream(
    window: tauri::Window,
    state: State<'_, AppCore>,
    message: String,
) -> Result<Message, String> {
    use tauri::Emitter;
    if message.trim().is_empty() {
        return Err("Cannot send an empty message.".into());
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let forward = tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = window.emit("chat-event", &ev);
        }
    });

    let mut turn = state.turn.lock().await;
    let result = {
        let crate::state::Turn { agent, ctx, session } = &mut *turn;
        agent.process_events(&message, ctx, session, tx).await
    };
    let _ = forward.await;
    if result.is_ok() {
        persist_session(&turn);
    }
    result.map(Message::assistant).map_err(|e| e.to_string())
}

/// Full transcript for hydrating the message list on load.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_history(state: State<'_, AppCore>) -> Result<Vec<Message>, String> {
    let turn = state.turn.lock().await;
    Ok(turn.session.messages().to_vec())
}

/// Sidebar/Status panel snapshot. All disk I/O happens before locking so a slow
/// read never holds the turn mutex and stalls an in-flight `send_message`.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_status(state: State<'_, AppCore>) -> Result<StatusResponse, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    // Best-effort memory metrics: a missing/locked DB must not fail the status
    // call. Use the configured data dir (open_in) so it matches where the
    // facts panel and the agent read/write memory.db.
    let memory_db_entries = match MemoryStore::open_in(&cfg.general.data_dir) {
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

/// Start a new conversation: persist the current one (if it has messages) and
/// reset the active session. NOTE: learned `UserModel` and daily-note markdown
/// that `Agent::process` already wrote to the data dir are intentionally kept.
#[tauri::command(rename_all = "snake_case")]
pub async fn clear_conversation(state: State<'_, AppCore>) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    persist_session(&turn);
    turn.session = Session::new("desktop", "local");
    Ok(())
}

/// New conversation — alias of `clear_conversation` with intent-revealing name
/// for the sidebar's "+ New chat" affordance.
#[tauri::command(rename_all = "snake_case")]
pub async fn new_conversation(state: State<'_, AppCore>) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    persist_session(&turn);
    turn.session = Session::new("desktop", "local");
    Ok(())
}

/// Conversation rows for the history sidebar (newest-updated first). Includes
/// the active session if it has unsaved messages, so it always appears.
#[tauri::command(rename_all = "snake_case")]
pub async fn list_conversations(state: State<'_, AppCore>) -> Result<Vec<ConversationMeta>, String> {
    let turn = state.turn.lock().await;
    let store = ConversationStore::open_default(&turn.agent.workspace_dir)
        .map_err(|e| e.to_string())?;
    store.list_meta().map_err(|e| e.to_string())
}

/// Switch the active conversation: persist the current one, load `id` into the
/// turn, and return its messages for the frontend to re-hydrate.
#[tauri::command(rename_all = "snake_case")]
pub async fn switch_conversation(
    state: State<'_, AppCore>,
    id: String,
) -> Result<Vec<Message>, String> {
    let mut turn = state.turn.lock().await;
    let store = ConversationStore::open_default(&turn.agent.workspace_dir)
        .map_err(|e| e.to_string())?;
    persist_session(&turn);
    match store.load(&id).map_err(|e| e.to_string())? {
        Some(session) => {
            let messages = session.messages().to_vec();
            turn.session = session;
            Ok(messages)
        }
        None => Err(format!("No conversation with id {id}")),
    }
}

/// Delete a conversation. If it is the active one, start a fresh session.
#[tauri::command(rename_all = "snake_case")]
pub async fn delete_conversation(state: State<'_, AppCore>, id: String) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    let store = ConversationStore::open_default(&turn.agent.workspace_dir)
        .map_err(|e| e.to_string())?;
    store.delete(&id).map_err(|e| e.to_string())?;
    if turn.session.id == id {
        turn.session = Session::new("desktop", "local");
    }
    Ok(())
}
