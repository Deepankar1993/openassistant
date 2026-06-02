// src/ui/web.rs
//! Web UI using Axum — browser-based chat interface
//! Serves HTML/JS frontend with WebSocket for real-time updates

use crate::ui::AppState;
use anyhow::Result;
use axum::{
    extract::{Query, State},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

// ─── Web API Types ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub success: bool,
    pub response: String,
    pub messages: Vec<WebMessage>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WebMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub model: String,
    pub mode: String,
    pub workspace: String,
    pub message_count: usize,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost: f64,
}

// ─── Web Server ───────────────────────────────────────────────────────

pub struct WebServer {
    pub state: Arc<Mutex<AppState>>,
    pub port: u16,
}

impl WebServer {
    pub fn new(port: u16) -> Self {
        Self {
            state: Arc::new(Mutex::new(AppState::default())),
            port,
        }
    }

    pub async fn run(&self) -> Result<()> {
        let state = self.state.clone();

        let app = Router::new()
            .route("/", get(serve_index))
            .route("/api/chat", post(handle_chat))
            .route("/api/status", get(handle_status))
            .route("/api/clear", post(handle_clear))
            .with_state(state);

        let addr: SocketAddr = format!("127.0.0.1:{}", self.port).parse()?;
        info!("Web UI starting on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

// ─── Handlers ─────────────────────────────────────────────────────────

async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn handle_chat(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(req): Json<ChatRequest>,
) -> Json<ChatResponse> {
    let mut state = state.lock().await;

    // Add user message
    state.add_message("user", &req.message);

    // Simulate agent response (in production, call the agent loop)
    let response = format!(
        "I received: \"{}\"\n\nThis is a simulated response. In production, this would run the full 11-step ReAct agent loop.",
        &req.message[..req.message.len().min(200)]
    );

    state.add_message("assistant", &response);

    let messages: Vec<WebMessage> = state
        .messages
        .iter()
        .map(|m| WebMessage {
            id: m.id.clone(),
            role: m.role.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp.to_rfc3339(),
        })
        .collect();

    Json(ChatResponse {
        success: true,
        response,
        messages,
    })
}

async fn handle_status(State(state): State<Arc<Mutex<AppState>>>) -> Json<StatusResponse> {
    let state = state.lock().await;
    Json(StatusResponse {
        model: state.model_name.clone(),
        mode: state.permission_mode.clone(),
        workspace: state.workspace_dir.clone(),
        message_count: state.messages.len(),
        tokens_in: state.total_input_tokens,
        tokens_out: state.total_output_tokens,
        cost: state.total_cost,
    })
}

async fn handle_clear(State(state): State<Arc<Mutex<AppState>>>) -> Json<serde_json::Value> {
    let mut state = state.lock().await;
    state.clear_messages();
    Json(serde_json::json!({ "success": true }))
}

/// Entry point for web mode
pub async fn run_web(port: u16) -> Result<()> {
    let server = WebServer::new(port);
    server.run().await
}

// ─── HTML Frontend ────────────────────────────────────────────────────

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>🦉 openAssistant</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: 'Segoe UI', system-ui, sans-serif; background: #1a1a2e; color: #eee; height: 100vh; display: flex; flex-direction: column; }
        .header { background: #16213e; padding: 12px 20px; display: flex; align-items: center; gap: 12px; border-bottom: 1px solid #0f3460; }
        .header h1 { font-size: 1.2em; color: #e94560; }
        .header .status { margin-left: auto; font-size: 0.85em; color: #888; }
        .main { display: flex; flex: 1; overflow: hidden; }
        .sidebar { width: 250px; background: #16213e; padding: 16px; border-right: 1px solid #0f3460; overflow-y: auto; }
        .sidebar h3 { color: #e94560; margin-bottom: 8px; font-size: 0.9em; }
        .sidebar .info { font-size: 0.85em; color: #aaa; margin-bottom: 4px; }
        .sidebar .info span { color: #fff; }
        .chat { flex: 1; display: flex; flex-direction: column; }
        .messages { flex: 1; overflow-y: auto; padding: 16px; }
        .message { margin-bottom: 16px; padding: 12px; border-radius: 8px; }
        .message.user { background: #0f3460; margin-left: 20%; }
        .message.assistant { background: #16213e; margin-right: 20%; border-left: 3px solid #e94560; }
        .message.system { background: #2a2a4a; font-size: 0.9em; color: #aaa; }
        .message .role { font-weight: bold; font-size: 0.8em; margin-bottom: 4px; }
        .message.user .role { color: #4cc9f0; }
        .message.assistant .role { color: #e94560; }
        .message .content { white-space: pre-wrap; word-wrap: break-word; }
        .input-area { padding: 16px; background: #16213e; border-top: 1px solid #0f3460; display: flex; gap: 8px; }
        .input-area input { flex: 1; padding: 12px; border: 1px solid #0f3460; border-radius: 8px; background: #1a1a2e; color: #fff; font-size: 1em; }
        .input-area input:focus { outline: none; border-color: #e94560; }
        .input-area button { padding: 12px 24px; background: #e94560; color: #fff; border: none; border-radius: 8px; cursor: pointer; font-weight: bold; }
        .input-area button:hover { background: #ff6b6b; }
        .input-area button:disabled { background: #444; cursor: not-allowed; }
        .typing { color: #888; font-style: italic; padding: 8px 16px; display: none; }
    </style>
</head>
<body>
    <div class="header">
        <h1>🦉 openAssistant</h1>
        <div class="status" id="status">Ready</div>
    </div>
    <div class="main">
        <div class="sidebar">
            <h3>Model</h3>
            <div class="info" id="model">openrouter/owl-alpha</div>
            <h3>Permission Mode</h3>
            <div class="info" id="mode">Default</div>
            <h3>Workspace</h3>
            <div class="info" id="workspace">-</div>
            <h3>Usage</h3>
            <div class="info">Tokens: <span id="tokens">0 / 0</span></div>
            <div class="info">Cost: <span id="cost">$0.00</span></div>
            <div class="info">Messages: <span id="msg-count">0</span></div>
        </div>
        <div class="chat">
            <div class="messages" id="messages"></div>
            <div class="typing" id="typing">🦉 Thinking...</div>
            <div class="input-area">
                <input type="text" id="input" placeholder="Type a message or /help for commands..." autocomplete="off">
                <button id="send" onclick="sendMessage()">Send</button>
            </div>
        </div>
    </div>
    <script>
        const messagesEl = document.getElementById('messages');
        const inputEl = document.getElementById('input');
        const sendBtn = document.getElementById('send');
        const typingEl = document.getElementById('typing');
        const statusEl = document.getElementById('status');

        inputEl.addEventListener('keypress', (e) => { if (e.key === 'Enter') sendMessage(); });

        async function sendMessage() {
            const msg = inputEl.value.trim();
            if (!msg) return;
            inputEl.value = '';
            sendBtn.disabled = true;
            typingEl.style.display = 'block';
            statusEl.textContent = 'Processing...';

            // Add user message immediately
            addMessage('user', msg);

            try {
                const res = await fetch('/api/chat', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ message: msg })
                });
                const data = await res.json();
                if (data.success) {
                    addMessage('assistant', data.response);
                }
            } catch (e) {
                addMessage('system', 'Error: ' + e.message);
            }

            typingEl.style.display = 'none';
            sendBtn.disabled = false;
            statusEl.textContent = 'Ready';
            updateStatus();
        }

        function addMessage(role, content) {
            const div = document.createElement('div');
            div.className = 'message ' + role;
            div.innerHTML = '<div class="role">' + role.toUpperCase() + '</div><div class="content">' + escapeHtml(content) + '</div>';
            messagesEl.appendChild(div);
            messagesEl.scrollTop = messagesEl.scrollHeight;
        }

        function escapeHtml(text) {
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }

        async function updateStatus() {
            try {
                const res = await fetch('/api/status');
                const data = await res.json();
                document.getElementById('model').textContent = data.model;
                document.getElementById('mode').textContent = data.mode;
                document.getElementById('workspace').textContent = data.workspace;
                document.getElementById('tokens').textContent = data.tokens_in + ' / ' + data.tokens_out;
                document.getElementById('cost').textContent = '$' + data.cost.toFixed(4);
                document.getElementById('msg-count').textContent = data.message_count;
            } catch (e) {}
        }

        updateStatus();
    </script>
</body>
</html>"#;
