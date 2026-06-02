# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
cargo build                 # debug build
cargo build --release       # release build
cargo run -- <subcommand>   # e.g. cargo run -- tui
cargo check                 # fast type-check without producing a binary
cargo clippy                # lints
```

Run a subcommand: `cargo run -- tui` (interactive terminal UI, also aliased as `chat`), `web --port 3000`, `gateway`, `onboard`, `status`, `doctor`, `config --key model.model --value ...`. See `Commands` enum in `src/main.rs` for the full list (memory, skills, agents, plugins, workflow, checkpoint).

### Tests
There is currently **no test suite** — no `tests/` directory and no `#[cfg(test)]` modules, despite `tempfile`/`tokio-test` being listed as dev-dependencies. Run a single test (once tests exist) with `cargo test <name>`.

## Important: docs describe a plan, not the code

`ARCHITECTURE.md` and parts of `README.md` describe an *aspirational* design (a Cargo **workspace** with `crates/`, 60+ tools, voice, companion apps). The actual project is a **single binary crate** with all modules under `src/`. Trust the source over those docs. Many advanced features are **scaffolded stubs** that return placeholder strings rather than doing real work — notably `goal_deliberate` and `task` (sub-agent) in `src/core/agent.rs` emit "In a full implementation..." text and do not actually call the LLM or spawn agents. Verify a feature is wired end-to-end before assuming it works.

## Architecture

openAssistant is a personal AI assistant. Everything flows through one agent loop that talks to an **OpenAI-compatible chat-completions API** (default provider OpenRouter, model `openrouter/owl-alpha`).

### The agent loop (`src/core/agent.rs`)
`Agent::process()` is the heart of the system:
1. Appends a daily note + adds the user message to the `Session`.
2. `FullContext::observe()` does naive keyword-based user-model learning.
3. Builds a system prompt from persona + user model + memory + a textual tool list.
4. Sends the last 30 messages to `POST {api_base}/chat/completions` (`call_llm`).
5. Parses the response for a tool call and dispatches it (`handle_tool_calls`).

**Tool calling is text-based, not native function calling.** The model is instructed to emit `[TOOL:name:{"arg":"value"}]`, and `parse_tool_call()` extracts it with a regex (`\[TOOL:(\w+):(\{.*?\})\]`). Only **one** tool call per turn is handled, and the tool output is appended to the assistant text — there is no multi-step tool/observe loop yet. When adding a tool you must update three places: the `match` in `handle_tool_calls`, the `default_tools()` list (for the prompt), and usually `ToolRegistry::execute` in `src/tools/mod.rs`.

### Context assembled into every prompt (`src/core/persona.rs`)
`FullContext` = `Persona` (the agent's identity/principles/boundaries, OpenClaw "SOUL.md" style) + `UserModel` (built up over time, Hermes "Honcho" style) + session stats. `build_system_prompt()` renders all of it to markdown.

### Two parallel memory systems — don't confuse them
- **File memory** (`src/core/memory.rs`, `MemoryWorkspace`): markdown files under the data dir — `MEMORY.md` (curated long-term), `memory/YYYY-MM-DD.md` (daily notes), `DREAMS.md`. This is what the agent loop reads/writes during conversation.
- **SQLite + FTS5** (`src/memory/store.rs`, `MemoryStore`): structured, full-text-searchable store at `memory.db`. Used by `status`/`doctor` and intended for session search.

### Module map (`src/`)
- `core/` — agent engine plus a large set of feature modules: `session`, `context`, `persona`, `memory`, `subagent`, `multi_agent`, `agent_teams`, `goal_system`, `workflows`, `channels`, `mcp`, `plugins`, `hooks`, `permissions`, `checkpoint`, `worktree`, `streaming`, `standing_orders`, `self_update`, `browser`, `web_search`. Several of these are Claude-Code-replica features in varying states of completeness.
- `tools/` — actual tool implementations: `bash`, `shell`, `file`, `file_search` (glob/grep), `browser`, `vision` (delegates to **Gemini CLI**, not an API).
- `gateway/` — messaging channels (`discord`, `telegram`, `slack`, `webchat`). `start_gateway()` currently only fully wires up WebChat; Discord/Telegram are mostly placeholders.
- `ui/` — `tui.rs` (ratatui), `web.rs` (axum server), `chat.rs`. The `tui`/`chat` CLI commands both call `ui::tui::run_tui()`.
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
