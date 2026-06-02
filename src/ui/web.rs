// src/ui/web.rs
//! Web UI using Axum — browser-based chat interface
//! Modern design inspired by OpenHumans: clean, card-based, with smooth interactions

use crate::ui::AppState;
use anyhow::Result;
use axum::extract::{Query, State};
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
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

    state.add_message("user", &req.message);

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

// ─── HTML Frontend — Modern Design Matching OpenHumans ────────────────
// Design principles:
// - Clean white background with subtle gray sections
// - Card-based layout with rounded corners and soft shadows
// - Blue primary (#2563eb) with orange/amber accents
// - Sans-serif font stack (Inter, system-ui)
// - Generous whitespace, smooth transitions
// - Mobile-responsive

const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>openAssistant — Your AI Companion</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&display=swap" rel="stylesheet">
    <style>
        /* ── Reset & Base ─────────────────────────────────────── */
        *, *::before, *::after { margin: 0; padding: 0; box-sizing: border-box; }

        :root {
            --primary: #2563eb;
            --primary-dark: #1d4ed8;
            --primary-light: #dbeafe;
            --accent: #f59e0b;
            --accent-light: #fef3c7;
            --success: #10b981;
            --danger: #ef4444;
            --bg-primary: #ffffff;
            --bg-secondary: #f8fafc;
            --bg-tertiary: #f1f5f9;
            --text-primary: #0f172a;
            --text-secondary: #475569;
            --text-muted: #94a3b8;
            --border: #e2e8f0;
            --border-light: #f1f5f9;
            --shadow-sm: 0 1px 2px rgba(0,0,0,0.05);
            --shadow-md: 0 4px 6px -1px rgba(0,0,0,0.07), 0 2px 4px -2px rgba(0,0,0,0.05);
            --shadow-lg: 0 10px 15px -3px rgba(0,0,0,0.08), 0 4px 6px -4px rgba(0,0,0,0.04);
            --shadow-xl: 0 20px 25px -5px rgba(0,0,0,0.08), 0 8px 10px -6px rgba(0,0,0,0.04);
            --radius-sm: 6px;
            --radius: 10px;
            --radius-lg: 16px;
            --radius-xl: 24px;
            --transition: 0.2s ease;
        }

        body {
            font-family: 'Inter', system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: var(--bg-secondary);
            color: var(--text-primary);
            line-height: 1.6;
            -webkit-font-smoothing: antialiased;
            -moz-osx-font-smoothing: grayscale;
            height: 100vh;
            overflow: hidden;
        }

        /* ── Header ───────────────────────────────────────────── */
        .header {
            background: var(--bg-primary);
            border-bottom: 1px solid var(--border);
            padding: 0 24px;
            height: 60px;
            display: flex;
            align-items: center;
            gap: 16px;
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            z-index: 100;
            box-shadow: var(--shadow-sm);
        }

        .header-logo {
            display: flex;
            align-items: center;
            gap: 10px;
            text-decoration: none;
        }

        .header-logo .logo-icon {
            width: 36px;
            height: 36px;
            background: linear-gradient(135deg, var(--primary), var(--accent));
            border-radius: var(--radius-sm);
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 18px;
            color: white;
            font-weight: 700;
        }

        .header-logo h1 {
            font-size: 1.15em;
            font-weight: 700;
            color: var(--text-primary);
            letter-spacing: -0.02em;
        }

        .header-logo h1 span {
            color: var(--primary);
        }

        .header-actions {
            margin-left: auto;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .header-btn {
            padding: 7px 14px;
            border: 1px solid var(--border);
            border-radius: var(--radius-sm);
            background: var(--bg-primary);
            color: var(--text-secondary);
            font-size: 0.85em;
            font-weight: 500;
            cursor: pointer;
            transition: all var(--transition);
            font-family: inherit;
        }

        .header-btn:hover {
            background: var(--bg-tertiary);
            border-color: var(--text-muted);
            color: var(--text-primary);
        }

        .header-btn.primary {
            background: var(--primary);
            color: white;
            border-color: var(--primary);
        }

        .header-btn.primary:hover {
            background: var(--primary-dark);
        }

        /* ── Layout ────────────────────────────────────────────── */
        .app-layout {
            display: flex;
            height: 100vh;
            padding-top: 60px;
        }

        /* ── Sidebar ───────────────────────────────────────────── */
        .sidebar {
            width: 280px;
            background: var(--bg-primary);
            border-right: 1px solid var(--border);
            display: flex;
            flex-direction: column;
            overflow-y: auto;
            flex-shrink: 0;
            transition: transform 0.3s ease;
        }

        .sidebar-section {
            padding: 20px;
            border-bottom: 1px solid var(--border-light);
        }

        .sidebar-section:last-child {
            border-bottom: none;
        }

        .sidebar-section h3 {
            font-size: 0.7em;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.08em;
            color: var(--text-muted);
            margin-bottom: 12px;
        }

        .sidebar-card {
            background: var(--bg-secondary);
            border: 1px solid var(--border-light);
            border-radius: var(--radius);
            padding: 14px;
            margin-bottom: 8px;
        }

        .sidebar-card .label {
            font-size: 0.75em;
            color: var(--text-muted);
            margin-bottom: 3px;
        }

        .sidebar-card .value {
            font-size: 0.9em;
            font-weight: 600;
            color: var(--text-primary);
            word-break: break-all;
        }

        .sidebar-card .value.highlight {
            color: var(--primary);
        }

        .stat-grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 8px;
        }

        .stat-item {
            background: var(--bg-secondary);
            border: 1px solid var(--border-light);
            border-radius: var(--radius-sm);
            padding: 10px;
            text-align: center;
        }

        .stat-item .stat-value {
            font-size: 1.2em;
            font-weight: 700;
            color: var(--primary);
        }

        .stat-item .stat-label {
            font-size: 0.7em;
            color: var(--text-muted);
            margin-top: 2px;
        }

        .new-chat-btn {
            width: 100%;
            padding: 12px;
            background: var(--primary);
            color: white;
            border: none;
            border-radius: var(--radius);
            font-size: 0.9em;
            font-weight: 600;
            cursor: pointer;
            transition: all var(--transition);
            font-family: inherit;
            display: flex;
            align-items: center;
            justify-content: center;
            gap: 8px;
        }

        .new-chat-btn:hover {
            background: var(--primary-dark);
            transform: translateY(-1px);
            box-shadow: var(--shadow-md);
        }

        /* ── Chat Area ─────────────────────────────────────────── */
        .chat-area {
            flex: 1;
            display: flex;
            flex-direction: column;
            min-width: 0;
            background: var(--bg-secondary);
        }

        /* ── Messages ──────────────────────────────────────────── */
        .messages {
            flex: 1;
            overflow-y: auto;
            padding: 24px 32px;
            scroll-behavior: smooth;
        }

        .messages::-webkit-scrollbar {
            width: 6px;
        }

        .messages::-webkit-scrollbar-track {
            background: transparent;
        }

        .messages::-webkit-scrollbar-thumb {
            background: var(--border);
            border-radius: 3px;
        }

        .message-group {
            display: flex;
            gap: 14px;
            margin-bottom: 24px;
            max-width: 800px;
        }

        .message-group.user {
            margin-left: auto;
            flex-direction: row-reverse;
        }

        .avatar {
            width: 38px;
            height: 38px;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 16px;
            flex-shrink: 0;
        }

        .avatar.assistant {
            background: linear-gradient(135deg, var(--primary), #7c3aed);
            color: white;
        }

        .avatar.user {
            background: linear-gradient(135deg, var(--accent), #f97316);
            color: white;
        }

        .avatar.system {
            background: var(--bg-tertiary);
            color: var(--text-muted);
        }

        .message-content {
            flex: 1;
            min-width: 0;
        }

        .message-header {
            display: flex;
            align-items: baseline;
            gap: 8px;
            margin-bottom: 6px;
        }

        .message-group.user .message-header {
            justify-content: flex-end;
        }

        .message-sender {
            font-weight: 600;
            font-size: 0.85em;
            color: var(--text-primary);
        }

        .message-time {
            font-size: 0.75em;
            color: var(--text-muted);
        }

        .message-bubble {
            background: var(--bg-primary);
            border: 1px solid var(--border);
            border-radius: var(--radius);
            border-top-left-radius: 2px;
            padding: 14px 18px;
            box-shadow: var(--shadow-sm);
            position: relative;
        }

        .message-group.user .message-bubble {
            background: var(--primary);
            border-color: var(--primary);
            color: white;
            border-radius: var(--radius);
            border-top-right-radius: 2px;
            border-top-left-radius: var(--radius);
        }

        .message-group.user .message-sender {
            color: var(--primary);
        }

        .message-bubble .content {
            white-space: pre-wrap;
            word-wrap: break-word;
            font-size: 0.95em;
            line-height: 1.7;
        }

        .message-group.user .message-bubble .content {
            line-height: 1.6;
        }

        /* ── Welcome Screen ────────────────────────────────────── */
        .welcome {
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            height: 100%;
            text-align: center;
            padding: 40px;
        }

        .welcome-icon {
            width: 80px;
            height: 80px;
            background: linear-gradient(135deg, var(--primary), var(--accent));
            border-radius: var(--radius-xl);
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 36px;
            margin-bottom: 24px;
            box-shadow: var(--shadow-lg);
        }

        .welcome h2 {
            font-size: 1.6em;
            font-weight: 700;
            margin-bottom: 8px;
            letter-spacing: -0.02em;
            color: var(--text-primary);
        }

        .welcome p {
            color: var(--text-secondary);
            font-size: 1em;
            max-width: 480px;
            margin-bottom: 32px;
        }

        .quick-actions {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 12px;
            max-width: 600px;
            width: 100%;
        }

        .quick-action {
            background: var(--bg-primary);
            border: 1px solid var(--border);
            border-radius: var(--radius);
            padding: 16px;
            cursor: pointer;
            transition: all var(--transition);
            text-align: left;
            font-family: inherit;
        }

        .quick-action:hover {
            border-color: var(--primary);
            box-shadow: var(--shadow-md);
            transform: translateY(-2px);
        }

        .quick-action .qa-icon {
            font-size: 20px;
            margin-bottom: 8px;
        }

        .quick-action .qa-title {
            font-weight: 600;
            font-size: 0.9em;
            color: var(--text-primary);
            margin-bottom: 4px;
        }

        .quick-action .qa-desc {
            font-size: 0.8em;
            color: var(--text-muted);
        }

        /* ── Input Area ─────────────────────────────────────────── */
        .input-area {
            padding: 20px 32px;
            background: var(--bg-primary);
            border-top: 1px solid var(--border);
        }

        .input-container {
            max-width: 800px;
            margin: 0 auto;
            position: relative;
        }

        .input-wrapper {
            display: flex;
            align-items: flex-end;
            gap: 12px;
            background: var(--bg-secondary);
            border: 2px solid var(--border);
            border-radius: var(--radius-lg);
            padding: 10px 10px 10px 18px;
            transition: all var(--transition);
        }

        .input-wrapper:focus-within {
            border-color: var(--primary);
            box-shadow: 0 0 0 3px var(--primary-light);
        }

        .input-wrapper textarea {
            flex: 1;
            border: none;
            outline: none;
            background: transparent;
            font-family: inherit;
            font-size: 0.95em;
            color: var(--text-primary);
            resize: none;
            max-height: 150px;
            line-height: 1.5;
            padding: 6px 0;
        }

        .input-wrapper textarea::placeholder {
            color: var(--text-muted);
        }

        .send-btn {
            width: 40px;
            height: 40px;
            background: var(--primary);
            color: white;
            border: none;
            border-radius: 50%;
            cursor: pointer;
            display: flex;
            align-items: center;
            justify-content: center;
            transition: all var(--transition);
            flex-shrink: 0;
        }

        .send-btn:hover {
            background: var(--primary-dark);
            transform: scale(1.05);
        }

        .send-btn:disabled {
            background: var(--text-muted);
            cursor: not-allowed;
            transform: none;
        }

        .input-hint {
            text-align: center;
            margin-top: 8px;
            font-size: 0.75em;
            color: var(--text-muted);
        }

        .input-hint kbd {
            background: var(--bg-tertiary);
            border: 1px solid var(--border);
            border-radius: 3px;
            padding: 1px 5px;
            font-size: 0.9em;
            font-family: inherit;
        }

        /* ── Typing Indicator ──────────────────────────────────── */
        .typing-indicator {
            display: flex;
            align-items: center;
            gap: 6px;
            padding: 14px 18px;
            background: var(--bg-primary);
            border: 1px solid var(--border);
            border-radius: var(--radius);
            border-top-left-radius: 2px;
            max-width: 100px;
            margin-bottom: 24px;
            box-shadow: var(--shadow-sm);
        }

        .typing-indicator .dot {
            width: 8px;
            height: 8px;
            background: var(--text-muted);
            border-radius: 50%;
            animation: typingBounce 1.4s infinite ease-in-out;
        }

        .typing-indicator .dot:nth-child(2) { animation-delay: 0.2s; }
        .typing-indicator .dot:nth-child(3) { animation-delay: 0.4s; }

        @keyframes typingBounce {
            0%, 60%, 100% { transform: translateY(0); opacity: 0.4; }
            30% { transform: translateY(-6px); opacity: 1; }
        }

        /* ── Scrollbar ─────────────────────────────────────────── */
        ::-webkit-scrollbar { width: 8px; }
        ::-webkit-scrollbar-track { background: transparent; }
        ::-webkit-scrollbar-thumb { background: var(--border); border-radius: 4px; }
        ::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }

        /* ── Responsive ────────────────────────────────────────── */
        @media (max-width: 900px) {
            .sidebar { display: none; }
            .messages { padding: 16px; }
            .input-area { padding: 16px; }
            .quick-actions { grid-template-columns: 1fr; }
        }

        /* ── Toast Notification ────────────────────────────────── */
        .toast {
            position: fixed;
            bottom: 24px;
            right: 24px;
            background: var(--text-primary);
            color: white;
            padding: 12px 20px;
            border-radius: var(--radius);
            font-size: 0.85em;
            box-shadow: var(--shadow-xl);
            z-index: 200;
            animation: slideUp 0.3s ease;
            display: none;
        }

        @keyframes slideUp {
            from { transform: translateY(20px); opacity: 0; }
            to { transform: translateY(0); opacity: 1; }
        }
    </style>
</head>
<body>

<!-- ── Header ─────────────────────────────────────────────────── -->
<header class="header">
    <a href="#" class="header-logo">
        <div class="logo-icon">🦉</div>
        <h1>open<span>Assistant</span></h1>
    </a>
    <div class="header-actions">
        <button class="header-btn" onclick="clearChat()" title="Clear conversation">✕ Clear</button>
        <button class="header-btn primary" onclick="location.reload()" title="New chat">+ New Chat</button>
    </div>
</header>

<!-- ── App Layout ─────────────────────────────────────────────── -->
<div class="app-layout">

    <!-- ── Sidebar ─────────────────────────────────────────── -->
    <aside class="sidebar">
        <div class="sidebar-section">
            <button class="new-chat-btn" onclick="clearChat()">
                <span>✚</span> New Conversation
            </button>
        </div>

        <div class="sidebar-section">
            <h3>Session Info</h3>
            <div class="sidebar-card">
                <div class="label">Model</div>
                <div class="value highlight" id="model">openrouter/owl-alpha</div>
            </div>
            <div class="sidebar-card">
                <div class="label">Permission Mode</div>
                <div class="value" id="mode">Default</div>
            </div>
            <div class="sidebar-card">
                <div class="label">Workspace</div>
                <div class="value" id="workspace" style="font-size: 0.8em;">-</div>
            </div>
        </div>

        <div class="sidebar-section">
            <h3>Usage Stats</h3>
            <div class="stat-grid">
                <div class="stat-item">
                    <div class="stat-value" id="tokens-in">0</div>
                    <div class="stat-label">Tokens In</div>
                </div>
                <div class="stat-item">
                    <div class="stat-value" id="tokens-out">0</div>
                    <div class="stat-label">Tokens Out</div>
                </div>
                <div class="stat-item">
                    <div class="stat-value" id="cost">$0</div>
                    <div class="stat-label">Cost</div>
                </div>
                <div class="stat-item">
                    <div class="stat-value" id="msg-count">0</div>
                    <div class="stat-label">Messages</div>
                </div>
            </div>
        </div>

        <div class="sidebar-section">
            <h3>Capabilities</h3>
            <div class="sidebar-card">
                <div class="value" style="font-size: 0.85em; line-height: 1.8;">
                    🔍 Web Search<br>
                    🌐 Browser Control<br>
                    📁 File Operations<br>
                    💻 Terminal Access<br>
                    🤖 Sub-Agents<br>
                    🔧 Tool Calling
                </div>
            </div>
        </div>
    </aside>

    <!-- ── Chat Area ───────────────────────────────────────── -->
    <main class="chat-area">
        <div class="messages" id="messages">

            <!-- Welcome Screen (shown when no messages) -->
            <div class="welcome" id="welcome">
                <div class="welcome-icon">🦉</div>
                <h2>Welcome to openAssistant</h2>
                <p>Your AI companion with terminal access, web search, browser control, and self-management capabilities. Ask me anything or try one of the quick actions below.</p>
                <div class="quick-actions">
                    <button class="quick-action" onclick="sendQuick('Help me organize my project files')">
                        <div class="qa-icon">📁</div>
                        <div class="qa-title">Organize Files</div>
                        <div class="qa-desc">Help structure and clean up project directories</div>
                    </button>
                    <button class="quick-action" onclick="sendQuick('Search the web for latest AI news')">
                        <div class="qa-icon">🔍</div>
                        <div class="qa-title">Web Search</div>
                        <div class="qa-desc">Search across 7 engines for real-time info</div>
                    </button>
                    <button class="quick-action" onclick="sendQuick('Open a website and summarize its content')">
                        <div class="qa-icon">🌐</div>
                        <div class="qa-title">Browse Web</div>
                        <div class="qa-desc">Control browser via CDP to visit pages</div>
                    </button>
                    <button class="quick-action" onclick="sendQuick('Run system diagnostics and report status')">
                        <div class="qa-icon">💻</div>
                        <div class="qa-title">System Check</div>
                        <div class="qa-desc">Run terminal commands and analyze output</div>
                    </button>
                </div>
            </div>

        </div>

        <!-- Input Area -->
        <div class="input-area">
            <div class="input-container">
                <div class="input-wrapper">
                    <textarea id="input" placeholder="Ask openAssistant anything..." rows="1"
                        onkeydown="handleKeyDown(event)"
                        oninput="autoResize(this)"></textarea>
                    <button class="send-btn" id="send" onclick="sendMessage()" aria-label="Send">
                        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M22 2L11 13"/><path d="M22 2L15 22L11 13L2 9L22 2Z"/>
                        </svg>
                    </button>
                </div>
                <div class="input-hint">
                    <kbd>Enter</kbd> to send · <kbd>Shift+Enter</kbd> for new line · <kbd>/help</kbd> for commands
                </div>
            </div>
        </div>
    </main>
</div>

<!-- Toast -->
<div class="toast" id="toast"></div>

<script>
    // ── State ────────────────────────────────────────────────────
    let isProcessing = false;

    // ── Auto-resize textarea ─────────────────────────────────────
    function autoResize(el) {
        el.style.height = 'auto';
        el.style.height = Math.min(el.scrollHeight, 150) + 'px';
    }

    // ── Keyboard handling ────────────────────────────────────────
    function handleKeyDown(e) {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            sendMessage();
        }
    }

    // ── Send message ─────────────────────────────────────────────
    async function sendMessage() {
        const input = document.getElementById('input');
        const msg = input.value.trim();
        if (!msg || isProcessing) return;

        isProcessing = true;
        input.value = '';
        input.style.height = 'auto';
        document.getElementById('send').disabled = true;

        // Hide welcome screen
        const welcome = document.getElementById('welcome');
        if (welcome) welcome.style.display = 'none';

        // Add user message
        addMessage('user', msg);
        showTyping(true);

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
            addMessage('system', '⚠ Connection error: ' + e.message);
        }

        showTyping(false);
        document.getElementById('send').disabled = false;
        isProcessing = false;
        updateStatus();
        input.focus();
    }

    // ── Quick action ─────────────────────────────────────────────
    function sendQuick(msg) {
        document.getElementById('input').value = msg;
        sendMessage();
    }

    // ── Add message to DOM ───────────────────────────────────────
    function addMessage(role, content) {
        const container = document.getElementById('messages');

        const group = document.createElement('div');
        group.className = 'message-group ' + role;

        const avatar = document.createElement('div');
        avatar.className = 'avatar ' + role;
        avatar.textContent = role === 'user' ? '👤' : (role === 'assistant' ? '🦉' : '⚙');

        const msgContent = document.createElement('div');
        msgContent.className = 'message-content';

        const header = document.createElement('div');
        header.className = 'message-header';

        const sender = document.createElement('span');
        sender.className = 'message-sender';
        sender.textContent = role === 'user' ? 'You' : (role === 'assistant' ? 'openAssistant' : 'System');

        const time = document.createElement('span');
        time.className = 'message-time';
        time.textContent = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

        header.appendChild(sender);
        header.appendChild(time);

        const bubble = document.createElement('div');
        bubble.className = 'message-bubble';
        const contentDiv = document.createElement('div');
        contentDiv.className = 'content';
        contentDiv.textContent = content;
        bubble.appendChild(contentDiv);

        msgContent.appendChild(header);
        msgContent.appendChild(bubble);

        group.appendChild(avatar);
        group.appendChild(msgContent);

        container.appendChild(group);
        container.scrollTop = container.scrollHeight;
    }

    // ── Typing indicator ─────────────────────────────────────────
    function showTyping(show) {
        let indicator = document.getElementById('typing-indicator');
        if (show) {
            if (!indicator) {
                indicator = document.createElement('div');
                indicator.id = 'typing-indicator';
                indicator.className = 'typing-indicator';
                indicator.innerHTML = '<div class="dot"></div><div class="dot"></div><div class="dot"></div>';
                document.getElementById('messages').appendChild(indicator);
            }
            indicator.style.display = 'flex';
            document.getElementById('messages').scrollTop = document.getElementById('messages').scrollHeight;
        } else if (indicator) {
            indicator.remove();
        }
    }

    // ── Update status ────────────────────────────────────────────
    async function updateStatus() {
        try {
            const res = await fetch('/api/status');
            const data = await res.json();
            document.getElementById('model').textContent = data.model;
            document.getElementById('mode').textContent = data.mode;
            document.getElementById('workspace').textContent = data.workspace;
            document.getElementById('tokens-in').textContent = formatNumber(data.tokens_in);
            document.getElementById('tokens-out').textContent = formatNumber(data.tokens_out);
            document.getElementById('cost').textContent = '$' + data.cost.toFixed(4);
            document.getElementById('msg-count').textContent = data.message_count;
        } catch (e) {}
    }

    // ── Clear chat ───────────────────────────────────────────────
    async function clearChat() {
        try {
            await fetch('/api/clear', { method: 'POST' });
        } catch (e) {}

        const container = document.getElementById('messages');
        container.innerHTML = '';

        // Restore welcome screen
        const welcomeHtml = `<div class="welcome" id="welcome">
            <div class="welcome-icon">🦉</div>
            <h2>New Conversation Started</h2>
            <p>How can I help you today? Try one of the quick actions below or type your own message.</p>
            <div class="quick-actions">
                <button class="quick-action" onclick="sendQuick('Help me organize my project files')">
                    <div class="qa-icon">📁</div>
                    <div class="qa-title">Organize Files</div>
                    <div class="qa-desc">Help structure and clean up project directories</div>
                </button>
                <button class="quick-action" onclick="sendQuick('Search the web for latest AI news')">
                    <div class="qa-icon">🔍</div>
                    <div class="qa-title">Web Search</div>
                    <div class="qa-desc">Search across 7 engines for real-time info</div>
                </button>
                <button class="quick-action" onclick="sendQuick('Open a website and summarize its content')">
                    <div class="qa-icon">🌐</div>
                    <div class="qa-title">Browse Web</div>
                    <div class="qa-desc">Control browser via CDP to visit pages</div>
                </button>
                <button class="quick-action" onclick="sendQuick('Run system diagnostics and report status')">
                    <div class="qa-icon">💻</div>
                    <div class="qa-title">System Check</div>
                    <div class="qa-desc">Run terminal commands and analyze output</div>
                </button>
            </div>
        </div>`;
        container.innerHTML = welcomeHtml;

        document.getElementById('msg-count').textContent = '0';
        updateStatus();
        showToast('Conversation cleared');
    }

    // ── Show toast ───────────────────────────────────────────────
    function showToast(message) {
        const toast = document.getElementById('toast');
        toast.textContent = message;
        toast.style.display = 'block';
        setTimeout(() => { toast.style.display = 'none'; }, 3000);
    }

    // ── Format number ────────────────────────────────────────────
    function formatNumber(n) {
        if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M';
        if (n >= 1000) return (n / 1000).toFixed(1) + 'K';
        return n.toString();
    }

    // ── Init ──────────────────────────────────────────────────────
    updateStatus();
    document.getElementById('input').focus();
</script>
</body>
</html>"##;
