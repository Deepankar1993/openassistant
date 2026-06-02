# Project Context

## Purpose
openAssistant is a personal AI assistant that unifies ideas from OpenHumans (personal
data ownership + modern UX), Hermes Agent (an evolving user model), and OpenClaw (a
persona/"SOUL.md" identity layer). Everything flows through a single agent loop that talks
to an OpenAI-compatible chat-completions API (default provider OpenRouter, model
`openrouter/owl-alpha`).

The near-term goal is a **cross-platform desktop application** that gives the assistant a
first-class native home on the desktop, visually and behaviorally matching the
**OpenHumans desktop** experience, while reusing the existing Rust agent core.

## Tech Stack
- **Language:** Rust (edition 2021), single binary crate — all modules under `src/`.
  Despite `ARCHITECTURE.md`, this is NOT a Cargo workspace.
- **Async runtime:** tokio (`#[tokio::main]`). Errors via `anyhow::Result`. Logging via `tracing`.
- **Existing UIs:**
  - TUI — `ratatui` + `crossterm` (`src/ui/tui.rs`, also the `chat` command).
  - Web — `axum` + `tower-http`, single embedded `INDEX_HTML` string (`src/ui/web.rs`).
- **Desktop (planned):** Tauri (cargo-tauri already installed) — reuses the Rust core as
  the backend, with a web frontend for the view layer.
- **Storage:** `rusqlite` (bundled) at `memory.db`; markdown memory files under the data dir.
- **HTTP:** `reqwest` to the chat-completions endpoint.
- **Frontend tooling available:** Node v22, npm.

## Project Conventions

### Code Style
- Tools return a `ToolResult { success, output, error }` and are invoked by string dispatch.
- Tool calling is **text-based**, not native function calling: the model emits
  `[TOOL:name:{"arg":"value"}]`, parsed by regex in `src/core/agent.rs`.
- Adding a tool touches three places: the `match` in `handle_tool_calls`, `default_tools()`,
  and usually `ToolRegistry::execute` in `src/tools/mod.rs`.
- Existing web aesthetic (the bar the desktop app must meet/exceed): Inter font; primary
  blue `#2563eb` / dark `#1d4ed8`; amber accent `#f59e0b`; success `#10b981`; clean
  card-based light theme with subtle borders (`#e2e8f0`) and soft backgrounds (`#f8fafc`).

### Architecture
- One agent loop (`Agent::process()` in `src/core/agent.rs`) is the heart of the system.
- `FullContext` = `Persona` + `UserModel` + session stats, rendered to a markdown system prompt.
- Two memory systems, kept distinct: file memory (`src/core/memory.rs`) and SQLite/FTS5
  (`src/memory/store.rs`).
- Many advanced modules are **scaffolded stubs** (e.g. `goal_deliberate`, `task` sub-agent
  return placeholder strings). Verify a feature is wired end-to-end before relying on it.

### Testing
- There is currently **no test suite** (no `tests/` dir, no `#[cfg(test)]` modules).
  New desktop work should introduce frontend E2E tests (Playwright) and Rust unit tests.

### Git Workflow
- Trunk-based on `main`. Professional, conventional-style commit messages.

## Domain Context
"OpenHumans aesthetic" = clean, modern, card-based, generous whitespace, friendly but
professional. The desktop app should feel native (tray, global shortcut, windowing) while
preserving this visual language and the assistant's persona-driven behavior.

## Important Constraints
- Reuse the existing Rust agent core; do not fork the agent loop for the desktop.
- Single-binary ethos — avoid heavyweight runtimes where a Rust-native option exists.
- Local-first / privacy-respecting: data stays under the user's data dir by default.

## External Dependencies
- OpenAI-compatible chat-completions API (default OpenRouter).
- Gemini CLI (vision tool delegates to it) and optional MCP servers.
