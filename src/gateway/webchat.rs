// src/gateway/webchat.rs
//! WebChat gateway — the HTTP messaging server. Runs the real agent loop
//! (`Agent::process`) and hosts the Slack Events endpoint on the same axum
//! server when Slack is configured.

use anyhow::Result;
use axum::{extract::State, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

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
        .route("/api/messages", get(list_messages).post(send_message));

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
        .with_tools_enabled(config.tools.enabled);
    GatewayState {
        agent: Arc::new(agent),
        web: Arc::new(Mutex::new(Convo::new("webchat", "web", &data_dir))),
        slack_sessions: Arc::new(Mutex::new(HashMap::new())),
        config: Arc::new(config),
    }
}

pub async fn start(config: Config) -> Result<()> {
    // Host/port resolve through the shared helpers (empty host ⇒ 0.0.0.0, port 0 ⇒ 3000).
    let host = crate::config::webchat_host(&config);
    let port = crate::config::webchat_port(&config);
    let state = build_state(config);
    let app = build_router(state);

    let addr = format!("{}:{}", host, port);
    info!("WebChat (real agent loop) listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index_handler() -> String {
    "openAssistant WebChat API is running. POST {\"content\":\"...\"} to /api/messages to chat.".to_string()
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
    Json(response)
}
