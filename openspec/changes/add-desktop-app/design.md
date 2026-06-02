# Design: add-desktop-app

## Context

openAssistant is a single binary crate (package `open-assistant`) whose working brain is the agent loop `Agent::process(&self, message, ctx: &mut FullContext, session: &mut Session) -> anyhow::Result<String>` in `src/core/agent.rs` (line 55). That loop is real and functional: it appends a daily note, adds the user message to the `Session`, runs `ctx.observe()`, builds a markdown system prompt from persona + user model + memory, POSTs to an OpenAI-compatible `/chat/completions` endpoint via `reqwest` (`call_llm`, line 156), runs single-shot text-based tool dispatch (`handle_tool_calls`, line 184), and returns the assistant string.

Critically, **neither shipped UI actually calls this loop.** `src/ui/web.rs` `handle_chat` returns a hardcoded `"This is a simulated response... 11-step ReAct agent loop"` string, and `src/ui/tui.rs` `send_message` returns `"In a full implementation, this would call the 11-step ReAct agent loop..."`. They are facades. The single most valuable thing this change does is wire `Agent::process` into a real UI — the desktop app must do what the existing UIs only pretend to do, not copy their dead wiring.

The toolchain is already installed: cargo-tauri 2.11.x (Tauri 2.x), Node v22, npm 11, rustc 1.96. `openspec/project.md` documents the OpenHumans aesthetic (Inter, primary `#2563eb`, dark `#1d4ed8`, amber `#f59e0b`, success `#10b981`, border `#e2e8f0`, bg `#f8fafc`), and that exact token set already exists verbatim in `src/ui/web.rs` `:root` (lines 172-186). That CSS is the real, controllable visual source of truth — far more reliable than the "OpenHumans desktop" reference, which the research itself admits is an unverified reconstruction of a third-party product (tinyhumansai's "OpenHuman") assembled from secondary blogs with no published palette.

This document is the technical design for adding a Tauri 2.x desktop shell that reuses the Rust core in-process. The expert panel's unanimous correction is reflected throughout: **scope is the killer.** The MVP is four things, not nine. Everything else is explicitly deferred.

## Goals / Non-Goals

### Goals (MVP / v1)

1. **lib/bin refactor** — add a `[lib]` target so `src-tauri` can `use open_assistant::...`. This is a hard prerequisite, landed as its own isolated commit before any Tauri code.
2. **One wired chat command** — a single async `#[tauri::command] send_message` that calls `Agent::process` against shared managed state, returning the real assistant `Message`. Non-streaming (returns one final `Message`, which is exactly what `Agent::process` already produces).
3. **Port the existing web.rs HTML/CSS** into the Tauri webview so the desktop matches the OpenHumans aesthetic without reinventing a palette.
4. **A settings/onboarding screen** that writes `model.api_key`, `model.model`, and `model.api_base`, with an API-key-missing gate on launch.

Cross-cutting v1 must-haves (cheap, high-payoff, demanded by the panel):
- **HTTP status check in `call_llm`** before `resp.json()` so a 401 from the empty default `api_key` surfaces as a real error instead of a silent blank bubble. (Benefits the CLI too.)
- **Single `tokio::sync::Mutex`** over a combined `{ ctx, session }` turn-state struct (NOT `std::sync::Mutex`, NOT two separate mutexes).
- **Locked-down CSP** (`default-src 'self'`, no `connect-src` to the LLM) and a **default-deny Tauri capabilities** posture.
- **A v1 tool-permission decision** (tools off by default / confirm-before-run) since `send_message` otherwise hands the model ungated `bash`/file/write on the host.
- **`data-testid` attributes** from day one + Rust `#[cfg(test)]` tests for the command/config layer.

### Non-Goals (explicitly NOT in v1 — ship a written "Not in v1" note with the PR)

These are deferred to P1/P2. Surfacing them now risks advertising stubbed core features as working desktop functionality.

- **LLM token streaming** (SSE). `Agent::process` returns one final string today; v1 uses a typing-dots indicator while the request is in flight. Streaming is a P1 fast-follow (design sketched below).
- **Conversation-history persistence** to SQLite. v1 session is in-memory and lost on restart (explicit decision). Do NOT build a new history schema and do NOT reuse `memory.db` (would pollute FTS memory search).
- **System tray, global hotkey / quick-launch, custom titlebar, window-state persistence, single-instance.** Each is a separate Tauri 2 plugin needing its own capability entry; budgeted as P1.
- **Memory browser** (read or edit), persona/user-model viewer.
- **Dark mode / theme toggle.** v1 ships the light theme only.
- **Animated multi-state mascot with voice lip-sync.** Cut entirely from v1. Voice is stubbed, there is no animation asset pipeline, and the feature is sourced from unverified secondary material. v1 ships the existing static owl with at most a 2-state CSS treatment (idle vs. an in-flight thinking spinner). Clearly labeled stretch goal.
- **Multi-agent / sub-agent (`task`) / goal deliberation / plan mode panels.** These handlers (`goal_deliberate` line 323, `task` line 370, `plan_mode` line 399, `perm` line 409) return placeholder text. They are NOT working features and must not appear in the desktop UI.
- **Provider profiles / multi-endpoint switching.** v1 exposes three plain fields (`api_key`, `model`, `api_base`) written to YAML. Named profiles are net-new modeling, deferred.
- **macOS native E2E.** No WKWebView WebDriver exists (see Testing). v1 promises native E2E on Windows + Linux only.

## Decisions

### D1 — Tauri 2.x, in-process, over Electron or a sidecar

**Decision:** Use Tauri 2.x with the Rust agent core linked **in-process** (no sidecar, no separate server). A new `src-tauri/` crate depends on the existing crate via a `[lib]` target.

**Rationale & the trade we are making (per Devil's Advocate, who correctly noted Tauri-vs-alternatives was asserted, not argued):**
- *Tauri in-process (chosen):* `Agent`, `FullContext`, `Session`, `config`, and the tool layer link as native Rust. No serialization across a process boundary, no second runtime to ship, no stdio/HTTP protocol to invent and version. Cost: the `[lib]`/bin refactor "touches every internal path," and we inherit OS WebView variance (WebView2 / WebKitGTK / WKWebView).
- *Electron / Node sidecar talking to the CLI over stdio or localhost HTTP (rejected):* would avoid the lib refactor and "reuse the core" via the binary, but (a) the CLI surface that actually runs the agent does not exist yet — both UI entry points are stubs, so there is no working stdio/HTTP agent server to wrap; (b) it ships a full Node/Chromium runtime, violating the single-binary ethos in `project.md`; (c) it reintroduces an IPC protocol and a 401/error-translation layer we would have to maintain. The "reuse" it offers is shallow.

The lib refactor is a one-time cost paid in an isolated commit; in-process linking is the lower long-run maintenance burden and matches the OpenHuman reference stack. Tauri wins.

**WebView constraint note (for reviewers):** The Tauri shell bundles the OS WebView (WebView2 on Windows, WebKitGTK on Linux, WKWebView on macOS). This is an **explicit, documented exception** to the "single-binary / avoid heavyweight runtimes" constraint in `project.md` — the OS WebView is a system component, not a bundled Chromium. Record this in `project.md`/the proposal so it is not flagged as a constraint violation.

### D2 — lib/bin refactor (the gating prerequisite)

**Decision:** Add a library target and re-export the modules the desktop needs; keep the existing clap CLI binary intact.

`Cargo.toml`:
```toml
[lib]
name = "open_assistant"
path = "src/lib.rs"

[[bin]]
name = "openassistant"
path = "src/main.rs"
```

New `src/lib.rs`:
```rust
pub mod config;
pub mod core;
pub mod gateway;
pub mod memory;
pub mod skills;
pub mod cron;
pub mod tools;
pub mod platforms;
pub mod canvas;
pub mod security;
pub mod onboarding;
pub mod ui;
```

`src/main.rs` drops its `mod` declarations (lines 2-13) and instead does `use open_assistant::{config, core, ui, gateway, ...};`. Internal absolute paths currently written as `crate::config::...` inside the modules continue to resolve under the lib crate unchanged (the lib *is* the crate root for those modules now), so no `crate::` → `super::` churn is needed within `src/`; only `main.rs` changes its reference root.

**This is landed FIRST, on its own branch, in an ISOLATED commit.** Acceptance gate before any Tauri work: `cargo build`, `cargo run -- tui`, `cargo run -- status`, `cargo run -- web` all pass. If the refactor is entangled with Tauri work, a build break becomes unbisectable.

### D3 — `src-tauri/` is a separate crate

**Decision:** Scaffold `src-tauri/` as its own crate that depends on the lib. Do NOT convert the root crate into the Tauri crate (root is a clap CLI and must stay one).

`src-tauri/Cargo.toml` (key deps):
```toml
[package]
name = "openassistant-desktop"
version = "0.1.0"
edition = "2021"

[lib]
name = "openassistant_desktop_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
open-assistant = { path = ".." }
tauri = { version = "2", features = [] }
tokio = { version = "1", features = ["sync", "rt-multi-thread", "macros"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
# v1 native-feature plugins are NOT added yet (see Non-Goals). When P1 lands:
# tauri-plugin-global-shortcut = "2"
# tauri-plugin-window-state    = "2"
# tauri-plugin-single-instance = "2"
```

### D4 — How the core is reused and exposed as commands

**Decision:** Hold a combined state struct in Tauri managed state and expose a minimal command surface.

State (in `src-tauri/src/state.rs`):
```rust
use open_assistant::core::agent::Agent;
use open_assistant::core::persona::FullContext;
use open_assistant::core::session::Session;

/// The mutable conversation turn-state, guarded as ONE unit.
pub struct TurnState {
    pub ctx: FullContext,
    pub session: Session,
}

pub struct DesktopState {
    /// Agent is Clone + cheap; not behind the turn lock.
    pub agent: Agent,
    /// SINGLE tokio mutex over the whole turn (see D5).
    pub turn: tokio::sync::Mutex<TurnState>,
}
```

Built in `tauri::Builder::setup` by loading config and constructing the agent:
```rust
let cfg = open_assistant::config::load().await?;       // async, file-based
let agent = Agent::new(cfg.model.model.clone())
    .with_workspace(cfg.general.data_dir.clone());
let state = DesktopState {
    agent,
    turn: tokio::sync::Mutex::new(TurnState {
        ctx: FullContext::new(),
        session: Session::default(),         // channel "cli", user "local"
    }),
};
app.manage(state);
```

Command surface (`src-tauri/src/commands.rs`), all `#[tauri::command]` async fns returning `Result<T, String>` (Tauri command errors must be `Serialize`; map via `.map_err(|e| e.to_string())`):

| Command | Signature (conceptual) | Behavior |
|---|---|---|
| `send_message` | `(State<DesktopState>, message: String) -> Result<Message, String>` | Locks `turn` once, calls `agent.process(&message, &mut t.ctx, &mut t.session).await`, returns the assistant `Message` (last entry of session). |
| `get_status` | `(State<DesktopState>) -> Result<StatusResponse, String>` | Mirrors `src/ui/web.rs` `StatusResponse`: model, permission_mode, workspace, message_count. See D11 re: token/cost. |
| `get_history` | `(State<DesktopState>) -> Result<Vec<Message>, String>` | Returns `session.messages.clone()`. |
| `clear_conversation` | `(State<DesktopState>) -> Result<(), String>` | Resets `session` to a fresh `Session::default()` and resets `ctx` to `FullContext::new()`. See D12 for side-effect semantics. |
| `load_config` | `() -> Result<ConfigView, String>` | Calls `config::load()`, returns a masked view (api_key shown as a boolean `has_api_key` + masked tail). |
| `save_config` | `(provider, model, api_base, api_key: Option<String>) -> Result<(), String>` | Loads `Config`, mutates struct fields **directly**, then `config::save()`. Does NOT route through `config::set()` (see D8). |

`Message` (`src/core/mod.rs`) is already `Serialize`/`Deserialize` (id/role/content/timestamp/metadata), so it crosses the IPC boundary as JSON with no adapter.

### D5 — Shared state & concurrency: one tokio mutex over the whole turn

**Decision:** A SINGLE `tokio::sync::Mutex<TurnState>` guards `ctx` + `session` as one unit, held across the entire `agent.process(...).await`.

- **`tokio::sync::Mutex`, not `std::sync::Mutex`** (Senior Dev correction): the guard is held across an `.await`. A `std::sync::MutexGuard` is `!Send` and will not compile in an async command. Use `let mut t = state.turn.lock().await;`.
- **One mutex, not two** (Industry Veteran + Devil's Advocate): locking `ctx` and `session` as separate mutexes risks deadlock ordering and interleaved session writes.
- **Turn lock is P0, not a footnote** (Devil's Advocate): a desktop user *will* press Enter twice. Without a whole-turn lock, the second call interleaves session writes and duplicates the daily-note/observe side effects. Holding the single lock for the whole `process()` call serializes turns; the frontend disables the send button while a request is in flight as a UX complement (not the safety mechanism — the lock is).

### D6 — Streaming via events: DEFERRED to P1 (design recorded, not built)

**v1:** non-streaming. `send_message` returns the final `Message`. The frontend shows typing dots while awaiting. This costs nothing and matches `Agent::process`'s current shape.

**P1 design (when streaming lands):** Do NOT use per-token `Window::emit`. The modern Tauri 2 idiom for ordered streaming data is `tauri::ipc::Channel<T>` (Senior Dev):
```rust
#[tauri::command]
async fn send_message_streamed(
    state: tauri::State<'_, DesktopState>,
    message: String,
    on_event: tauri::ipc::Channel<StreamEvent>,
) -> Result<(), String> { /* call refactored streaming process; on_event.send(delta) per chunk */ }
```
Reuse the existing `src/core/streaming.rs` `StreamEvent` enum as the wire schema (add `#[serde(rename_all = "camelCase")]`) so CLI NDJSON and desktop share one model. This requires refactoring `call_llm` to set `"stream": true` and consume `reqwest`'s SSE byte stream — net-new async plumbing with its own error model, explicitly out of v1.

### D7 — Frontend stack

**Decision:** Ship the simplest thing that ports the existing design and supports E2E: a static `dist/` of vanilla HTML/CSS/JS served from `src-tauri/`'s `frontendDist`, seeded by porting `src/ui/web.rs` `INDEX_HTML` (the `:root` token block lines 172-186 and the card/sidebar/bubble CSS) into `src-tauri/ui/index.html` + `styles.css` + `app.js`. `app.js` calls `import { invoke } from '@tauri-apps/api/core'` for `send_message`, `get_status`, etc.

- A bundler (Vite) is optional and may be added if the team prefers modules; v1 does not require a framework (no React). Keeping it framework-free minimizes the WebView-variance surface and the E2E setup.
- `tauri.conf.json` `build.frontendDist` points at the built `dist/`; `build.devUrl` points at a dev server (or a `python -m http.server`/`vite` on a fixed port) so Playwright can drive the same HTML in a plain Chromium during development (see Testing).

### D8 — Settings/config wiring: bypass `config::set()`

**Decision:** Settings writes load the `Config`, mutate struct fields directly, then call `config::save()`. They do NOT go through `config::set()`.

**Why (verified in `src/config/mod.rs` lines 158-172):** `config::set()` only handles `model.provider`, `model.model`, `model.api_key`, the three gateway tokens, and `security.dm_pairing`. Any other key — including `model.api_base` — hits the `_ => tracing::warn!("Unknown config key")` arm and **silently writes nothing**. A settings screen saving `api_base` through `set()` would appear to succeed and do nothing. So `save_config` does:
```rust
let mut cfg = open_assistant::config::load().await.map_err(|e| e.to_string())?;
cfg.model.provider = provider;
cfg.model.model    = model;
cfg.model.api_base = api_base;
if let Some(k) = api_key { cfg.model.api_key = k; }   // only overwrite if provided
open_assistant::config::save(&cfg).await.map_err(|e| e.to_string())?;
```
After a settings save, rebuild/replace the in-state `Agent` so the new model takes effect immediately (or rely on the fact that `call_llm` re-reads config per turn — see D11; for v1 the per-turn reload means `api_base`/`api_key`/`model`-via-config changes apply on the next message without restart, but the `Agent.model` field set at construction does not, so explicitly refresh `Agent::new(cfg.model.model)` on save).

### D9 — System tray + global shortcut: DEFERRED to P1

Not in v1 (see Non-Goals). When added, each is a distinct Tauri 2 plugin requiring a capability permission entry (Senior Dev):
- `tauri-plugin-global-shortcut` (register a configurable hotkey, default e.g. `CmdOrCtrl+Shift+Space`, in `setup` via `app.global_shortcut().register(...)`).
- Tray via Tauri's built-in `tray-icon` API (menu: Open / Quick Ask / Quit).
- `tauri-plugin-window-state` for geometry persistence.
- `tauri-plugin-single-instance` (must be registered FIRST if deep-linking is ever added).

Each needs its own entry in `src-tauri/capabilities/default.json`. Budgeting them as separate P1 items keeps the MVP honest.

### D10 — Security: capabilities, CSP, and tool gating

**CSP (`tauri.conf.json` `app.security.csp`):** Tauri 2 ships CSP **disabled** by default — it is only enforced if set (Senior Dev). Because the **Rust backend** makes the LLM HTTP call (`call_llm`), the webview needs no outbound network. Lock it down:
```json
"security": {
  "csp": "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'"
}
```
Do NOT add the OpenRouter / LLM origin to `connect-src` — the webview never talks to it. The `https://fonts.googleapis.com` font `<link>` in the ported HTML must be removed and Inter shipped as a local `@font-face` (self-hosted woff2 under the frontend dist) so `default-src 'self'` holds; otherwise relax `font-src`/`style-src` minimally rather than opening `connect-src`.

**Capabilities (`src-tauri/capabilities/default.json`):** default-DENY posture. Grant only `core:default` plus the specific permissions actually used. No blanket grants. P1 plugin permissions (global-shortcut, tray, window-state) are added to this file only when those features land.
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

**Tool gating (release blocker, not a "concern" — Devil's Advocate):** `send_message` hands the LLM working `bash -c` and write-anywhere on the host, and the `perm`/`plan_mode` handlers enforce nothing (placeholder text). Tauri capabilities do NOT help here because the tools run inside the Rust core, not via Tauri plugins — gating must be added in `src/core/agent.rs`, not in capabilities. v1 decision:
- **Tools are OFF by default in the desktop build.** `DesktopState` carries a `tools_enabled: bool` (default `false`); when false, `handle_tool_calls` is bypassed and the raw assistant text is returned. A settings toggle ("Enable file & shell tools — advanced") flips it, gated behind a one-time consent dialog.
- When enabled, the first invocation of `bash`/`write`/`edit` in a session raises a **confirm-before-run** dialog (a `confirm_tool(call) -> bool` command the frontend answers) before the tool executes. Crude but ships; far better than the cosmetic `permission_mode` field.
- This also sidesteps the **Windows bash defect** for the default demo path (see D13): with tools off, the broken `bash` invocation is never reached.

### D11 — `call_llm` robustness + the token/cost telemetry honesty fix

**HTTP status check (P0, ~5 lines, benefits CLI too):** `call_llm` (lines 175-180) currently does `resp.json()` with no status check and `content...unwrap_or("")`. A 401 from the empty default `api_key` yields an empty string the UI renders as a blank assistant bubble — a silent failure. Add before `resp.json()`:
```rust
let status = resp.status();
if !status.is_success() {
    let body = resp.text().await.unwrap_or_default();
    anyhow::bail!("LLM API error {}: {}", status, body);
}
```
This propagates a real error string up through `Agent::process` → `send_message` → the frontend error toast.

**Token/cost telemetry (honesty fix — Devil's Advocate):** the core does NOT compute `tokens_in`/`tokens_out`/`cost` today, so a status surface that shows them displays permanent zeros. v1 decision: **remove token/cost fields from the desktop `StatusResponse`** (show only model, mode, workspace, message_count). Capturing the provider's `usage` block in `call_llm` and threading it into status is a P1 enhancement, not a v1 promise.

**Per-turn config reload (acceptable for v1):** `call_llm` calls `config::load().await` every turn (file I/O per message). This is fine functionally and conveniently means `api_base`/`api_key` settings edits apply on the next message. Caching `Config` in managed state with invalidation-on-save is a P1 optimization, noted not done.

### D12 — Side-effect & persistence semantics for "clear" / "new session"

**Decision, made explicit (Devil's Advocate):** `Agent::process` has persistent side effects beyond the in-memory session — it appends to `memory/YYYY-MM-DD.md` daily notes and mutates the `UserModel` via `ctx.observe()` on every message. Therefore:
- `clear_conversation` resets the in-memory `Session` and `FullContext` **only**. It does NOT delete daily notes or rewind learned user-model state. The UI label is "Clear chat" (not "Erase memory") to set the right expectation.
- v1 session is in-memory; closing the app loses the conversation thread (but the daily-note markdown the agent wrote persists on disk under the data dir). This is the documented v1 behavior; durable conversation history is a Non-Goal.

### D13 — The Windows `bash` defect (confronted, not deferred)

**Verified:** `src/tools/bash.rs` line 55 does `tokio::process::Command::new("bash").arg("-c")` and the file's header comment calling it "sandboxed" (line 2) is inaccurate — there is no sandbox. On Windows 11 (the primary target per env), `bash` is not guaranteed on `PATH`.

**v1 handling:** because tools are OFF by default in the desktop build (D10), the flagship demo never hits this path. When a user opts into tools, `bash::execute` is wrapped with an availability check; on Windows with no `bash`, route to PowerShell (`Command::new("powershell").args(["-NoProfile","-Command", cmd])`) or return a clear "bash not available on this platform" error rather than a spawn failure. The permanent fix (platform-aware shell selection + real sandboxing) is tracked separately; v1 only needs it to not crash the demo machine.

### D14 — Tool-call protocol defect (scoped out of v1, recorded)

**Verified defect (Devil's Advocate):** `parse_tool_call` (line 546) uses regex `\[TOOL:(\w+):(\{.*?\})\]`. The non-greedy `\{.*?\}` matches up to the **first** `}`, so any tool whose arguments contain a nested object or brace — `write` with braces in `content`, `todo_write`'s array, `goal_deliberate` — truncates and fails `serde_json::from_str`, silently degrading to empty args (`unwrap_or(json!({}))`). The system prompt instructs the model to emit exactly those nested payloads. This is a real bug, not a future limitation.

**v1 handling:** since tools are off by default (D10), the desktop MVP does not depend on tool-call parsing. We do NOT surface tools as a headline feature. If/when tools are enabled, fixing the regex (e.g. balanced-brace matching or switching to native function-calling) is a prerequisite — tracked, not in the MVP critical path.

### D15 — Data & config locations

Unchanged from the CLI; the desktop shares them so CLI and desktop see one assistant:
- Config: `~/.openassistant/config.yaml` (`config_path()`; `data_dir` resolves via `$HOME`/`$USERPROFILE` + `/.openassistant`, `src/config/mod.rs` lines 129-133). `model.api_key` ships **empty** — the launch gate (D4/onboarding) handles this.
- Data dir: `~/.openassistant/` — `MEMORY.md`, `memory/`, `memory.db`, `skills/`. `Agent::with_workspace(cfg.general.data_dir)` points the desktop at it.
- Tauri's own bundle/config lives under `src-tauri/` (`tauri.conf.json`, `capabilities/`, `icons/`, `gen/`) and is unrelated to the assistant data dir.

## Architecture diagram

```
+---------------------------------------------------------------+
|                  Desktop App (Tauri 2.x process)              |
|                                                               |
|  +-----------------------------+   +-----------------------+  |
|  |  WebView (OS-native)        |   |  Rust backend         |  |
|  |  WebView2 / WKWebView /     |   |  (openassistant-      |  |
|  |  WebKitGTK                  |   |   desktop crate)      |  |
|  |                             |   |                       |  |
|  |  index.html + styles.css    |   |  #[tauri::command]    |  |
|  |  app.js                     |   |   send_message        |  |
|  |  (ported from web.rs        |   |   get_status          |  |
|  |   INDEX_HTML :root tokens)  |   |   get_history         |  |
|  |  data-testid hooks          |   |   clear_conversation  |  |
|  |                             |   |   load/save_config    |  |
|  |  CSP: default-src 'self'    |   |                       |  |
|  |  (NO connect-src to LLM)    |   |  managed State:       |  |
|  |                             |   |   DesktopState {      |  |
|  |   invoke('send_message') ---+-->|     agent: Agent,     |  |
|  |   <--- Result<Message>      |   |     turn: tokio       |  |
|  |                             |   |       ::Mutex<        |  |
|  +-----------------------------+   |        TurnState{ctx, |  |
|         capabilities/default.json  |        session}>,     |  |
|         (default-deny, core:default)|    tools_enabled }   |  |
|                                    +-----------+-----------+  |
+------------------------------------------------|--------------+
                                                 | in-process call
                                                 v
                +--------------------------------------------------+
                |   open_assistant  [lib]  (src/lib.rs)            |
                |                                                  |
                |   core::agent::Agent::process(msg, ctx, session)|
                |     |- mem.append_daily()    (side effect)      |
                |     |- ctx.observe()          (user-model learn)|
                |     |- build_system_prompt(persona+model+mem)   |
                |     |- call_llm()  --[reqwest POST]------------->+--> OpenAI-compatible
                |     |    (+ resp.status() check, NEW)           |     /chat/completions
                |     |- handle_tool_calls()                      |     (OpenRouter default)
                |     |    (BYPASSED when tools_enabled=false)    |
                |     '- returns final String                     |
                |                                                  |
                |   config::{load,save}   memory   tools   ...     |
                +--------------------------------------------------+
                                                 ^
                                                 | also linked by
                +--------------------------------------------------+
                |   openassistant  [[bin]]  (src/main.rs)          |
                |   clap CLI: tui / web / status / ... (UNCHANGED) |
                +--------------------------------------------------+

Shared on disk:  ~/.openassistant/  (config.yaml, MEMORY.md, memory/, memory.db)
```

## Testing strategy

The panel's strongest correction here: **macOS has no native WebView WebDriver**, so cross-platform native E2E parity is impossible via the official path. We commit to a three-layer strategy and do NOT promise native macOS E2E.

**Layer 1 — Rust unit/integration tests (everywhere, fast, CI-friendly) — the load-bearing layer.**
In `src-tauri/src/` `#[cfg(test)]` modules and/or `src-tauri/tests/`:
- Config round-trip: `save_config` mutates struct + `config::save()`, then `config::load()` reflects `api_base`/`model`/`api_key` (the exact thing `config::set()` would silently drop — regression-guards D8).
- `send_message` happy path with a mocked/stubbed LLM endpoint (point `api_base` at a local `wiremock`/`httpmock` server) returns a `Message` with `role == "assistant"`.
- `call_llm` error surfacing: a 401 from the mock yields an `Err` whose string contains the status (regression-guards D11), NOT an empty success.
- Turn-lock serialization: two concurrent `send_message` calls do not interleave session writes (assert session message ordering/count).
- These run on Windows, Linux, and macOS in CI.

**Layer 2 — Web-frontend UI tests in plain Chromium (Playwright) (cross-platform UI logic).**
Run the ported HTML against the `devUrl` dev server (not the native shell) in headless Chromium. Smoke flows: app loads, type into `[data-testid="chat-input"]`, click `[data-testid="send-btn"]`, a reply bubble renders in `[data-testid="message-list"]`; open `[data-testid="settings-btn"]`, fill `[data-testid="settings-api-key"]`, save. The frontend's `invoke` calls are stubbed (inject a `window.__TAURI_INTERNALS__` mock or a thin fetch shim) so Playwright tests pure UI behavior without the native bridge. `data-testid` attributes are added from day one on: chat input, send button, message list, message bubble, settings button, and each settings field.

**Layer 3 — Native smoke E2E via tauri-driver + WebdriverIO (Windows + Linux ONLY).**
Official Tauri 2 E2E is WebDriver via `tauri-driver` (WebdriverIO or Selenium), which works on Windows (Edge WebView2 + `msedgedriver`) and Linux (`WebKitWebDriver`) but **NOT macOS**. A handful of native smoke tests (app boots, single message round-trips against a mock endpoint) gated to Windows + Linux CI. Raw Playwright can only attach to WebView2 on Windows via CDP remote-debugging args; we prefer `tauri-driver` for the native path to keep one tool.

**Explicitly not promised:** native macOS E2E. macOS is covered by Layers 1 and 2 only. This is documented so reviewers don't expect parity that the platform cannot provide.

## Migration / rollout

1. **Branch `refactor/lib-bin-split`.** Add `[lib]`/`[[bin]]` + `src/lib.rs`; update `main.rs` to `use open_assistant::*`. Gate: `cargo build` + `cargo run -- {tui,web,status}` green. Merge as an isolated commit. (No Tauri code in this PR.)
2. **Branch `feat/desktop-scaffold`.** `cargo tauri init` into `src-tauri/`; add the lib dependency; set `tauri.conf.json` (`frontendDist`, `devUrl`, `app.security.csp`, window `main`); add `capabilities/default.json` (default-deny). App launches a blank window. No behavior yet.
3. **Branch `feat/desktop-chat`.** Add `DesktopState`, `send_message` + status/history/clear commands, the single tokio turn mutex, the `call_llm` status-check fix, and `tools_enabled=false` default. Wire the ported `index.html`/`styles.css`/`app.js`. Land Layer-1 Rust tests.
4. **Branch `feat/desktop-settings`.** Settings screen + `load_config`/`save_config` (direct struct mutation, D8) + API-key-missing launch gate + the tools opt-in/consent toggle (D10). Land Layer-2 Playwright UI tests.
5. **Ship v1** with the written **"Not in v1"** note (streaming, history persistence, tray, global hotkey, memory browser, dark mode, mascot, multi-agent/sub-agent/goal/plan panels, macOS native E2E). Tag native E2E (Layer 3) to Win/Linux CI.
6. **P1 backlog (separate changes):** SSE streaming via `Channel<StreamEvent>`; tray + global-shortcut + window-state plugins (+ capabilities); read-only memory browser; dark theme; token/cost telemetry from provider `usage`; tool-call regex fix + platform-aware shell.

The CLI binary and all its subcommands keep working unchanged throughout — the lib refactor is additive, and the desktop is a separate crate.

## Risks & mitigations

| # | Risk | Mitigation |
|---|------|------------|
| R1 | **Scope creep mislabeled as MVP** (the unanimous panel verdict). The research lists ~9 "P0" items plus a voiced mascot — a quarter's roadmap, not a minimum. | Hard-cut v1 to four items (lib refactor, one `send_message`, ported UI, settings). Ship the written "Not in v1" note (Migration step 5). Get the cut agreed before code. |
| R2 | **Team reskins a stub and demos a non-functional app** — both `web.rs`/`tui.rs` return literal "simulated" strings. | Wiring `Agent::process` is P0 step 3 and is the first behavioral acceptance gate; Layer-1 test asserts a real assistant `Message`. Do not copy stub wiring. |
| R3 | **No `[lib]` target blocks the whole reuse goal.** | D2; isolated first commit with a build gate before any Tauri work. Unbisectable build breaks avoided. |
| R4 | **Empty default `api_key` → silent 401** rendered as a blank bubble. | `call_llm` status check (D11) + API-key-missing launch gate routing to settings (D4). First-run looks like "sign in," not "broken." |
| R5 | **`config::set()` silently no-ops `api_base`** (warn-only `_` arm, verified). | `save_config` bypasses `set()` and mutates the struct directly (D8); Layer-1 round-trip test guards it. |
| R6 | **`std::sync::Mutex` won't compile across `.await`; two mutexes deadlock.** | Single `tokio::sync::Mutex<TurnState>` (D5). |
| R7 | **Ungated LLM shell/file access on a packaged desktop = RCE liability** (Devil's Advocate: release blocker). Tauri capabilities don't help — tools run in the core. | Tools OFF by default in the desktop build; opt-in behind consent + confirm-before-run dialog (D10). Gating added in `agent.rs`, not capabilities. |
| R8 | **Windows has no `bash`; `bash.rs` "sandboxed" comment is false.** | Tools-off default avoids the path for the demo; availability check + PowerShell fallback when opted in (D13). |
| R9 | **Tool-call regex truncates nested JSON** (verified defect). | Not in the MVP critical path (tools off). Tools not surfaced as a headline feature; regex fix is a P1 prerequisite to enabling them (D14). |
| R10 | **OpenHumans target is an unverified secondary-source reconstruction; no published palette.** | Anchor on the controllable `web.rs` `:root` tokens (verified at lines 172-186) as the source of truth; treat OpenHuman parity as "spirit, not pixel-match" (D7). |
| R11 | **Mascot over-scoping** — voice is stubbed, no asset pipeline, sourced from blogs. | Cut from v1 entirely; static owl + 2-state CSS only. Labeled stretch goal (Non-Goals). |
| R12 | **Token/cost telemetry the core never computes** → permanent zeros presented as real. | Remove token/cost from desktop status in v1; capture provider `usage` in P1 (D11). |
| R13 | **CSP is OFF by default in Tauri 2; webview might be over-permissioned.** | Explicitly set `default-src 'self'` with no `connect-src` to the LLM (the backend makes the call); self-host Inter to keep `'self'` (D10). |
| R14 | **macOS native E2E is impossible** (no WKWebView WebDriver). | Three-layer strategy; macOS covered by Rust + Chromium UI tests; native E2E gated to Win/Linux. Documented, not promised (Testing). |
| R15 | **OS WebView variance** (WebView2/WebKitGTK/WKWebView) causes per-platform rendering/behavior bugs. | Framework-free static frontend minimizes surface; CI runs Layer-1 everywhere and Layer-3 on Win/Linux; manual smoke on each target before release. |
| R16 | **"Clear conversation" silently leaves daily-note + user-model state** the user thinks they cleared. | Explicit semantics (D12): label "Clear chat"; reset in-memory state only; document that daily notes persist. |
| R17 | **Per-turn `config::load()` file I/O** + mid-session config drift. | Acceptable for v1 (conveniently applies settings edits next turn); refresh `Agent::new` on save; cache-with-invalidation deferred to P1 (D11). |
