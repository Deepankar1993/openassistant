// src/gateway/webchat.rs
//! WebChat gateway — the HTTP messaging server. Runs the real agent loop
//! (`Agent::process`) and hosts the Slack Events endpoint on the same axum
//! server when Slack is configured.

use anyhow::Result;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse};
use axum::{extract::State, routing::{delete, get, post}, Json, Router};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::core::agent::AgentEvent;
use crate::core::conversation_store::{ConversationMeta, ConversationStore};

use crate::config::Config;
use crate::core::agent::Agent;
use crate::core::persona::{FullContext, Persona};
use crate::core::session::Session;

/// Shared state for every gateway HTTP route. `Agent` is shared read-only;
/// conversations are isolated per surface (the web UI shares one session;
/// Slack keys sessions by channel).
#[derive(Clone)]
pub struct GatewayState {
    pub agent: Arc<Agent>,
    pub config: Arc<Config>,
    pub web: Arc<Mutex<Convo>>,
    pub slack_sessions: Arc<Mutex<HashMap<String, Convo>>>,
}

/// One conversation: the learned context + message history.
pub struct Convo {
    pub ctx: FullContext,
    pub session: Session,
    pub messages: Vec<ChatMessage>,
}

impl Convo {
    pub fn new(channel: &str, user: &str, data_dir: &str) -> Self {
        let mut ctx = FullContext::new();
        ctx.persona = Persona::load_or_default(data_dir);
        Self {
            ctx,
            session: Session::new(channel, user),
            messages: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

impl ChatMessage {
    fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.to_string(),
            content: content.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct SendMessage {
    content: String,
}

/// Build the gateway HTTP router. The Slack Events route is mounted only when
/// Slack is configured (signing secret present).
pub fn build_router(state: GatewayState) -> Router {
    let mut app = Router::new()
        .route("/", get(index_handler))
        .route("/api/messages", get(list_messages).post(send_message))
        .route("/api/chat/stream", post(chat_stream))
        .route("/api/conversations", get(list_conversations).post(new_conversation))
        .route("/api/conversations/select", post(select_conversation))
        .route("/api/conversations/{id}", delete(delete_conversation))
        .route("/vendor/marked.min.js", get(|| async { js(VENDOR_MARKED) }))
        .route("/vendor/purify.min.js", get(|| async { js(VENDOR_PURIFY) }))
        .route("/vendor/highlight.min.js", get(|| async { js(VENDOR_HLJS) }))
        .route("/vendor/hljs-github.min.css", get(|| async { css(VENDOR_HLJS_LIGHT) }))
        .route("/vendor/hljs-github-dark.min.css", get(|| async { css(VENDOR_HLJS_DARK) }));

    if !state.config.gateway.slack_signing_secret.is_empty()
        || !state.config.gateway.slack_token.is_empty()
    {
        app = app.route("/slack/events", post(super::slack::events_handler));
        info!("Slack Events endpoint mounted at POST /slack/events");
    }

    app.with_state(state)
}

/// Construct the shared gateway state (one agent built from config).
pub fn build_state(config: Config) -> GatewayState {
    let data_dir = config.general.data_dir.clone();
    let agent = Agent::new(config.model.model.clone())
        .with_workspace(data_dir.clone())
        .with_tools_enabled(config.tools.enabled)
        .with_permission_mode(crate::core::permissions::PermissionMode::from_str(
            &config.permissions.gateway_mode,
        ));
    GatewayState {
        agent: Arc::new(agent),
        web: Arc::new(Mutex::new(Convo::new("webchat", "web", &data_dir))),
        slack_sessions: Arc::new(Mutex::new(HashMap::new())),
        config: Arc::new(config),
    }
}

pub async fn start(config: Config) -> Result<()> {
    start_on(config, None).await
}

/// Start the WebChat server, optionally overriding the configured port (used
/// by the `web` CLI command's `--port` flag).
pub async fn start_on(config: Config, port_override: Option<u16>) -> Result<()> {
    // Host/port resolve through the shared helpers (empty host ⇒ 0.0.0.0, port 0 ⇒ 3000).
    let host = crate::config::webchat_host(&config);
    let port = port_override.unwrap_or_else(|| crate::config::webchat_port(&config));
    let state = build_state(config);
    let app = build_router(state);

    let addr = format!("{}:{}", host, port);
    info!("WebChat (real agent loop) listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Static assets (vendored, embedded in the binary) ──

const WEBCHAT_PAGE: &str = include_str!("webchat_page.html");
const VENDOR_MARKED: &str = include_str!("../../frontend/vendor/marked.min.js");
const VENDOR_PURIFY: &str = include_str!("../../frontend/vendor/purify.min.js");
const VENDOR_HLJS: &str = include_str!("../../frontend/vendor/highlight.min.js");
const VENDOR_HLJS_LIGHT: &str = include_str!("../../frontend/vendor/hljs-github.min.css");
const VENDOR_HLJS_DARK: &str = include_str!("../../frontend/vendor/hljs-github-dark.min.css");

fn js(body: &'static str) -> impl IntoResponse {
    ([("content-type", "application/javascript; charset=utf-8")], body)
}

fn css(body: &'static str) -> impl IntoResponse {
    ([("content-type", "text/css; charset=utf-8")], body)
}

async fn index_handler() -> Html<&'static str> {
    Html(WEBCHAT_PAGE)
}

/// Streaming chat: runs the agent turn in a background task and forwards each
/// `AgentEvent` as one SSE `data:` line of JSON. The web conversation lock is
/// held by the task for the whole turn (same serialization as `send_message`).
async fn chat_stream(
    State(state): State<GatewayState>,
    Json(payload): Json<SendMessage>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

    tokio::spawn(async move {
        let mut guard = state.web.lock().await;
        let convo = &mut *guard;
        convo.messages.push(ChatMessage::new("user", payload.content.clone()));
        // process_events emits Done/Error itself; a dropped receiver (client
        // hit Stop / disconnected) just makes sends no-ops.
        if let Ok(reply) = state
            .agent
            .process_events(&payload.content, &mut convo.ctx, &mut convo.session, tx)
            .await
        {
            convo.messages.push(ChatMessage::new("assistant", reply));
        }
        // Persist the turn (user message + any assistant reply, including a
        // partial/errored one — the user's message is already in the session).
        // Snapshot then drop the lock so the synchronous SQLite write stays off
        // the turn's critical section.
        let snapshot = convo.session.clone();
        drop(guard);
        persist_convo(&state.config, &snapshot);
    });

    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(|ev| {
        Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into())))
    });
    Sse::new(stream)
}

async fn list_messages(State(state): State<GatewayState>) -> Json<Vec<ChatMessage>> {
    let convo = state.web.lock().await;
    Json(convo.messages.clone())
}

async fn send_message(
    State(state): State<GatewayState>,
    Json(payload): Json<SendMessage>,
) -> Json<ChatMessage> {
    // Single web conversation: hold the lock for the whole turn so concurrent
    // posts can't interleave session writes (mirrors the desktop's per-turn
    // lock). tokio::sync::Mutex's guard is Send, so this is fine across await.
    let mut guard = state.web.lock().await;
    // Reborrow the guard as `&mut Convo` so the borrow checker can split the
    // disjoint `ctx`/`session` field borrows (it can't through MutexGuard's Deref).
    let convo = &mut *guard;
    convo.messages.push(ChatMessage::new("user", payload.content.clone()));

    let reply = match state
        .agent
        .process(&payload.content, &mut convo.ctx, &mut convo.session)
        .await
    {
        Ok(r) => r,
        Err(e) => format!("⚠️ {}", e),
    };

    let response = ChatMessage::new("assistant", reply);
    convo.messages.push(response.clone());
    // Snapshot + drop the lock before the synchronous SQLite write.
    let snapshot = convo.session.clone();
    drop(guard);
    persist_convo(&state.config, &snapshot);
    Json(response)
}

// ── Conversation history ──

/// Best-effort persistence of the active web conversation (empty sessions are
/// skipped, so the sidebar has no blank rows). A failed save never breaks a turn.
fn persist_convo(config: &Config, session: &Session) {
    if session.messages().is_empty() {
        return;
    }
    match ConversationStore::open_default(&config.general.data_dir) {
        Ok(store) => {
            if let Err(e) = store.save(session, None) {
                warn!("could not persist web conversation: {}", e);
            }
        }
        Err(e) => warn!("could not open conversation store: {}", e),
    }
}

/// Rebuild the display message list from a loaded session.
fn to_chat_messages(session: &Session) -> Vec<ChatMessage> {
    session
        .messages()
        .iter()
        .map(|m| ChatMessage {
            id: m.id.clone(),
            role: m.role.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp.to_rfc3339(),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct SelectBody {
    id: String,
}

async fn list_conversations(State(state): State<GatewayState>) -> Json<Vec<ConversationMeta>> {
    match ConversationStore::open_default(&state.config.general.data_dir) {
        Ok(store) => Json(store.list_meta().unwrap_or_default()),
        Err(_) => Json(Vec::new()),
    }
}

/// Start a new conversation: persist the current one and reset the active convo.
async fn new_conversation(State(state): State<GatewayState>) -> Json<serde_json::Value> {
    let mut guard = state.web.lock().await;
    persist_convo(&state.config, &guard.session);
    *guard = Convo::new("webchat", "web", &state.config.general.data_dir);
    Json(serde_json::json!({ "ok": true }))
}

/// Switch the active conversation; returns the loaded messages for re-render.
async fn select_conversation(
    State(state): State<GatewayState>,
    Json(body): Json<SelectBody>,
) -> Result<Json<Vec<ChatMessage>>, (StatusCode, String)> {
    let mut guard = state.web.lock().await;
    persist_convo(&state.config, &guard.session);
    let store = ConversationStore::open_default(&state.config.general.data_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match store
        .load(&body.id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    {
        Some(session) => {
            let messages = to_chat_messages(&session);
            guard.messages = messages.clone();
            guard.session = session;
            Ok(Json(messages))
        }
        None => Err((StatusCode::NOT_FOUND, format!("No conversation with id {}", body.id))),
    }
}

async fn delete_conversation(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    // Lock first, then delete inside the critical section: otherwise a turn
    // racing between the delete and the lock would re-persist (un-delete) the row.
    let mut guard = state.web.lock().await;
    if let Ok(store) = ConversationStore::open_default(&state.config.general.data_dir) {
        let _ = store.delete(&id);
    }
    if guard.session.id == id {
        *guard = Convo::new("webchat", "web", &state.config.general.data_dir);
    }
    Json(serde_json::json!({ "ok": true }))
}
