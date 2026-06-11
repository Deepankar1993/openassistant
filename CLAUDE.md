# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
cargo build                 # debug build (CLI binary + open_assistant lib)
cargo build --release       # release build
cargo build --workspace     # also builds the src-tauri desktop crate
cargo run -- <subcommand>   # e.g. cargo run -- tui
cargo check                 # fast type-check without producing a binary
cargo clippy                # lints
```

Run a subcommand: `cargo run -- tui` (interactive terminal UI, also aliased as `chat`), `web --port 3000`, `gateway`, `brief` (print the daily brief now), `onboard`, `status`, `doctor`, `config --key model.model --value ...`. See `Commands` enum in `src/main.rs` for the full list (memory, skills, agents, plugins, workflow, checkpoint).

### Desktop app (Tauri 2.x)
The desktop app lives in `src-tauri/` (crate `openassistant-desktop`) with a static frontend in `frontend/`. It reuses the agent core **in-process** via the `open_assistant` library target — it does NOT shell out to the CLI.

```bash
cargo tauri dev             # run the desktop app (from repo root; --no-watch to disable file-watch)
cargo tauri build           # produce a bundled installer
```

First run routes to a **4-screen onboarding wizard** (Workspace → AI Provider → Permissions → Finish) until an API key is set (the default key ships empty); it is re-enterable from Settings. Tool execution (shell/file access) is **off by default**, persisted in a `[tools]` config section, and gated behind an opt-in toggle.

**Desktop internals to know:**
- Commands live in `src-tauri/src/commands/` (one module per domain: `chat`, `settings`, `onboarding`, `memory`, `skills`, `system`), registered through a **single** `tauri::generate_handler!` in `lib.rs` (Tauri keeps only the last `invoke_handler` — never call it twice).
- Plugins in use: `tauri-plugin-dialog` (folder picker) and `tauri-plugin-opener` (external provider-doc links, scoped to openrouter.ai/openai.com in `capabilities/default.json`). `probe_connection` calls the LLM via `reqwest` from Rust (no http plugin/capability needed).
- All config writes use load→mutate→`config::save()`, never `config::set()` (whose allowlist silently drops most keys).
- `lib.rs` carries a **CAPABILITY HONESTY TABLE**: stub core features (sub-agent/workflow execution, checkpoint restore, plugin marketplace, self-update, skill activation, live gateway) MUST NOT get a working UI affordance. The desktop surfaces only verified-real features: Chat, full Settings, Memory browser, Skills manager, Status/Doctor diagnostics; `list_agents` is read-only.
- Frontend (`frontend/app.js`) talks to Tauri via `invoke` with **snake_case** arg keys matching the Rust params, and falls back to an in-app `defaultMock` (and a Playwright `installMock`) when run in a plain browser.

### Tests
Tests now exist (run a single one with `cargo test <name>`):
- **Rust unit tests** — `cargo test --workspace`. Covers config YAML round-trip + defaults (`src/config/mod.rs`), permissions (wildcard rules, mode checks), the agent's permission/truncation/sub-agent helpers (`src/core/agent.rs`), and the desktop command layer (`src-tauri/src/commands.rs`). All tests pass.
- **Playwright E2E** — `cd tests/e2e && npm install && npx playwright test`. Drives the static frontend in Chromium against an injectable mock backend (`window.__MOCK_BACKEND__`), so UI logic is testable on every OS including macOS (which has no native WKWebView WebDriver). Native `tauri-driver` smoke tests are a Windows/Linux-only follow-up.

## Important: docs describe a plan, not the code

`ARCHITECTURE.md` and parts of `README.md` describe an *aspirational* design (a Cargo workspace with `crates/`, 60+ tools, voice, companion apps). The actual project is a **two-member Cargo workspace**: the root crate (`open-assistant`) exposes both the `open_assistant` **library** (`src/lib.rs`) and the CLI **binary** (`src/main.rs`), and `src-tauri/` is the desktop app crate that path-depends on the root. (The root was a single binary crate before the `add-desktop-app` change; the `[lib]` split lets the desktop reuse the core in-process.) Trust the source over the aspirational docs. Some advanced features are still **scaffolded stubs** that return placeholder strings rather than doing real work — e.g. `plan_mode` and `perm` in `src/core/agent.rs`, plus `agent_teams`, `channels`, `standing_orders`, hooks fire points, MCP tool invocation, cron scheduling, and streaming. (`goal_deliberate` and `task` are real now: deliberation makes per-role LLM calls, and `task` spawns an in-process sub-agent.) Verify a feature is wired end-to-end before assuming it works. The desktop app deliberately surfaces only the wired chat path and does NOT expose stubs as working features.

## Architecture

openAssistant is a personal AI assistant. Everything flows through one agent loop that talks to an **OpenAI-compatible chat-completions API** (default provider OpenRouter, model `openrouter/owl-alpha`).

### The agent loop (`src/core/agent.rs`)
`Agent::process()` is the heart of the system:
1. Appends a daily note + adds the user message to the `Session`.
2. `FullContext::observe()` does naive keyword-based user-model learning.
3. Builds a system prompt from persona + user model + memory + a textual tool list.
4. Sends the last 30 messages to `POST {api_base}/chat/completions` (`call_llm`).
5. Loops: parses the response for a tool call, permission-checks it, executes it (`execute_tool`), feeds the result back to the model as a `[TOOL RESULT: name]` message, and calls the LLM again — until the model answers without a tool call or `MAX_TOOL_ITERATIONS` (6) is hit. Each tool output is truncated to 16 KiB before re-entering context.

`Agent::process_events()` runs the same loop but streams `AgentEvent`s (Token/ToolStart/ToolEnd/Done/Error) over an mpsc sender — used by WebChat SSE and the desktop's `send_message_stream` command. The JSON event shape is a frozen contract shared by both frontends.

**Tool calling is text-based, not native function calling.** The model is instructed to emit `[TOOL:name:{"arg":"value"}]`, and `parse_tool_call()` extracts it with a regex (`\[TOOL:(\w+):(\{.*?\})\]`) — one tool call per model message, but multiple rounds per turn via the loop above. When adding a tool you must update three places: the `match` in `execute_tool`, the `default_tools()` list (for the prompt), and usually `ToolRegistry::execute` in `src/tools/mod.rs`.

**Permissions are enforced in the loop (origin-aware).** `Agent.permission_mode` defaults to `BypassPermissions` for local front-ends (TUI/desktop keep full autonomy once tools are enabled); gateway channels construct agents with `permissions.gateway_mode` from config (default `acceptEdits`). Config `[permissions]` `allow`/`ask`/`deny` rules (Claude-Code-style, incl. `Bash(git *)` wildcards) apply at **every** mode — deny beats bypass; `Ask` resolves to a refusal text returned to the model (the agent is headless). The `task` tool spawns a real in-process sub-agent with a scoped tool list and depth limit 1.

### Context assembled into every prompt (`src/core/persona.rs`)
`FullContext` = `Persona` (the agent's identity/principles/boundaries, OpenClaw "SOUL.md" style) + `UserModel` (built up over time, Hermes "Honcho" style) + session stats. `build_system_prompt()` renders all of it to markdown.

### Two parallel memory systems — don't confuse them
- **File memory** (`src/core/memory.rs`, `MemoryWorkspace`): markdown files under the data dir — `MEMORY.md` (curated long-term), `memory/YYYY-MM-DD.md` (daily notes), `DREAMS.md`. This is what the agent loop reads/writes during conversation.
- **SQLite + FTS5** (`src/memory/store.rs`, `MemoryStore`): structured, full-text-searchable store at `memory.db`. Used by `status`/`doctor` and intended for session search.

### Module map (`src/`)
- `core/` — agent engine plus a large set of feature modules: `session`, `context`, `persona`, `memory`, `subagent`, `multi_agent`, `agent_teams`, `goal_system`, `workflows`, `channels`, `mcp`, `plugins`, `hooks`, `permissions`, `checkpoint`, `worktree`, `streaming`, `standing_orders`, `self_update`, `browser`, `web_search`. Several of these are Claude-Code-replica features in varying states of completeness.
- `tools/` — actual tool implementations: `bash`, `shell`, `file`, `file_search` (glob/grep), `browser`, `vision` (delegates to **Gemini CLI**, not an API).
- `gateway/` — messaging channels (`discord`, `telegram`, `slack`, `webchat`). `start_gateway()` wires up **all four** to the real agent loop (Slack is served by the WebChat axum server). Discord persists sessions/threads in `discord.db` and can route turns through the Claude Code CLI bridge (`src/core/claude_bridge.rs`); Telegram/Slack sessions are in-memory (lost on restart). WebChat serves a full single-file UI (`gateway/webchat_page.html`, embedded via `include_str!`) with SSE streaming at `POST /api/chat/stream` — events are `AgentEvent` JSON (`token`/`tool_start`/`tool_end`/`done`/`error`, the same contract the desktop `chat-event` Tauri events use). Vendored JS libs (marked, DOMPurify, highlight.js) live in `frontend/vendor/` and are embedded/served at `/vendor/*`. The gateway also spawns a **proactive loop** (`gateway/proactive.rs`, 60s tick, re-reads config live): delivers the daily brief (`core/brief.rs`, config `[brief]`) at the configured local time and checks URL watchers (`core/watchers.rs`, managed via the `watch` agent tool, state in `<data_dir>/proactive.json`), posting to the Discord home channel and/or a configured Telegram chat.
- `ui/` — `tui.rs` (ratatui), `chat.rs`. The `tui`/`chat` CLI commands both call `ui::tui::run_tui()`. The old `ui/web.rs` (a demo page that returned hardcoded "simulated response" text) was removed — the `web` CLI command now starts the gateway WebChat server (real agent loop, `--port` honored).
- `config/` — `Config` struct ↔ `~/.openassistant/config.yaml` (YAML via serde). `data_dir_default()` resolves `$HOME`/`$USERPROFILE` + `/.openassistant`.
- `skills/`, `cron/`, `platforms/` (OpenHumans-style data sources), `canvas/`, `security/` (DM pairing + allowlist), `onboarding/` (setup wizard).

### Config & data locations
- Config: `~/.openassistant/config.yaml` — auto-created with defaults on first `config::load()`.
- Data dir: `~/.openassistant/` — holds `MEMORY.md`, `memory/`, `memory.db`, `skills/`.
- Claude-Code-style assets are read from `<data_dir>/.claude/` (`agents/`, `plugins/`).
- Set values via `cargo run -- config --key <k> --value <v>`; only the keys in `config::set()` are settable that way (others require editing the YAML).

## Conventions
- Async runtime is **tokio** (`#[tokio::main]`); most I/O is `async`. Errors use `anyhow::Result`. Logging via `tracing` (`RUST_LOG`-style filter set in `main.rs`).
- Tool implementations return a `ToolResult { success, output, error }` (or a tool-specific result struct) and are invoked through string dispatch — keep new tools consistent with that shape.
