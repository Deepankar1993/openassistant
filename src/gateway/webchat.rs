// src/gateway/webchat.rs
use anyhow::Result;
use axum::{extract::State, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

#[derive(Clone)]
struct AppState {
    messages: Arc<Mutex<Vec<ChatMessage>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    id: String,
    role: String,
    content: String,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
struct SendMessage {
    content: String,
}

pub async fn start(port: u16) -> Result<()> {
    let state = AppState {
        messages: Arc::new(Mutex::new(Vec::new())),
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/messages", get(list_messages).post(send_message))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("WebChat listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index_handler() -> String {
    "openAssistant WebChat API is running. POST to /api/messages to chat.".to_string()
}

async fn list_messages(State(state): State<AppState>) -> Json<Vec<ChatMessage>> {
    let msgs = state.messages.lock().await;
    Json(msgs.clone())
}

async fn send_message(
    State(state): State<AppState>,
    Json(payload): Json<SendMessage>,
) -> Json<ChatMessage> {
    let msg = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: payload.content.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    // Store user message
    {
        let mut msgs = state.messages.lock().await;
        msgs.push(msg.clone());
    }

    // Generate assistant response (simplified)
    let response = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: format!("Echo: {}", payload.content),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    {
        let mut msgs = state.messages.lock().await;
        msgs.push(response.clone());
    }

    Json(response)
}
