# Implementation Tasks: add-desktop-app

Cross-platform Tauri 2.x desktop app for openAssistant that reuses the existing Rust agent core in-process. Items marked **[MVP]** are the agreed minimum viable product (per Industry Veteran / Devil's Advocate cut). Everything else is P1/P2 fast-follow. Do not start P1 work until all **[MVP]** items are checked and `cargo build` + `cargo run -- tui|status|web` still pass.

> **Scope guardrail (reconciled from panel):** v1 ships a working, non-streaming chat in an OpenHumans-styled shell with an API-key gate. The following are explicitly **NOT in v1**: LLM token streaming, conversation-history persistence to SQLite, system tray, global hotkey, memory browser, dark mode, the animated/voiced mascot, and any multi-agent / sub-agent / goal-deliberation / plan-mode panels (those core handlers are placeholder stubs — never surface them as working). See task 6.7.

---

## Phase 1 — Scaffold (lib/bin refactor + Tauri crate)

> Do this phase as its own isolated branch/commit. Get `cargo build` and `cargo run -- tui|status|web` green BEFORE adding any Tauri code, so a later Tauri break stays bisectable.

- [ ] 1.1 **[MVP]** Add a `[lib]` target to root `Cargo.toml`: `[lib]` `name = "open_assistant"` `path = "src/lib.rs"` (keep the existing `[[bin]]`/default bin). Verify `cargo metadata` shows both targets.
- [ ] 1.2 **[MVP]** Create `src/lib.rs` re-exporting the modules src-tauri needs: `pub mod core; pub mod config; pub mod tools; pub mod memory; pub mod skills; pub mod ui;` (plus `gateway/cron/platforms/canvas/security/onboarding` if referenced). Match the `mod` set currently in `src/main.rs` lines 2-13.
- [ ] 1.3 **[MVP]** Change `src/main.rs` to consume the lib: replace the `mod ...;` block with `use open_assistant::{core, config, tools, ui, ...};` so the CLI binary builds against the library, not its own copy of the modules.
- [ ] 1.4 **[MVP]** Confirm no path breakage from the refactor: run `cargo build`, `cargo clippy`, and smoke-run `cargo run -- status`, `cargo run -- tui` (quit immediately), `cargo run -- web --port 3000` (curl `/`). All must pass. Commit this as the isolated refactor commit.
- [ ] 1.5 **[MVP]** Scaffold the desktop crate with `cargo tauri init` into `src-tauri/` (or hand-create), giving it `package.name = "openassistant-desktop"`. Do NOT convert the root crate into the Tauri crate — keep the clap CLI intact.
- [ ] 1.6 **[MVP]** In `src-tauri/Cargo.toml`, add the local dep `open-assistant = { path = ".." }` plus `tauri = { version = "2", features = [] }`, `tauri-build` (build-dep), `tokio`, `serde`, `serde_json`. Run `cargo build` inside `src-tauri/` to confirm the core links in-process.
- [ ] 1.7 **[MVP]** Create the frontend project under `src-tauri/` (e.g. `src-tauri/../frontend` or a `dist`-emitting Vite app): `package.json` with Node 22 / npm, a minimal `index.html`, and `@tauri-apps/api` v2. Wire `tauri.conf.json` `build.frontendDist` / `build.devUrl` to it.
- [ ] 1.8 **[MVP]** Set `tauri.conf.json` `app.windows[0]` to a sensible default (title "openAssistant", `width: 1100`, `height: 760`, `minWidth: 860`, `minHeight: 560`, `resizable: true`).
- [ ] 1.9 **[MVP]** Set an explicit, locked-down CSP in `tauri.conf.json` `app.security.csp` (Tauri 2 ships CSP **off** by default). Because the Rust core makes the LLM HTTP call (not the webview), use e.g. `"default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost"` with **no** LLM/OpenRouter origin in `connect-src`.
- [ ] 1.10 **[MVP]** Create `src-tauri/capabilities/default.json` with a default-DENY posture: window label `main`, `permissions: ["core:default"]` only. Add per-plugin permissions later as features land (tasks 6.x). Confirm `cargo tauri dev` launches a blank window.
- [ ] 1.11 Add a short note to `openspec/project.md` (or the change's design doc) recording the decision that the Tauri-bundled OS WebView (WebView2/WebKitGTK/WKWebView) is an explicit, accepted exception to the "single binary" constraint, and that Tauri-in-process was chosen over an Electron/sidecar approach because the core is a Rust crate that links directly.

## Phase 2 — Core bridge (agent loop → Tauri commands)

> The single most important phase: both existing UIs (`web.rs` `handle_chat`, `tui.rs` `send_message`) return hardcoded "simulated"/"In a full implementation..." strings and never call the agent. Do NOT copy their wiring — call `Agent::process` directly.

- [ ] 2.1 **[MVP]** Define managed state in `src-tauri/src/state.rs`: a single struct guarded by **one** `tokio::sync::Mutex` (NOT `std::sync::Mutex` — its guard is `!Send` and won't compile across `.await`). E.g. `struct AppCore { agent: Agent, turn: tokio::sync::Mutex<Turn> }` where `struct Turn { ctx: FullContext, session: Session }`. One lock guards a whole turn to prevent interleaved session writes / duplicated daily-note side effects from double-Enter.
- [ ] 2.2 **[MVP]** Build the state in `tauri::Builder::setup`: `config::load().await`, construct `Agent::new(config.model.model).with_workspace(config.general.data_dir)`, init `FullContext::new()` and `Session::new("desktop", "local")`, then `.manage(AppCore { ... })`.
- [ ] 2.3 **[MVP]** Implement `#[tauri::command] async fn send_message(state, message: String) -> Result<Message, String>`: lock `turn`, call `agent.process(&message, &mut turn.ctx, &mut turn.session).await`, return the assistant `Message` (the core's `Message` is already `Serialize`/`Deserialize`). Map errors with `.map_err(|e| e.to_string())`.
- [ ] 2.4 **[MVP]** Add an HTTP status check in `src/core/agent.rs` `call_llm` BEFORE `resp.json()`: check `resp.status().is_success()`, and on failure `anyhow::bail!` with the status code + (truncated) response body. This fixes the silent blank-bubble failure the default empty `api_key` currently produces (and benefits the CLI too). Verify a deliberately-bad key returns a real error string to the command.
- [ ] 2.5 **[MVP]** Implement `#[tauri::command] async fn get_status(state) -> StatusResponse` mirroring `src/ui/web.rs` `StatusResponse` (model, mode, workspace, message_count). Decide token/cost: either capture `usage` from the LLM response in `call_llm` and populate `tokens_in/out`, OR omit those fields so the UI never shows permanent zeros as if real (per Devil's Advocate). Document which was chosen.
- [ ] 2.6 **[MVP]** Implement `#[tauri::command] async fn get_history(state) -> Vec<Message>` returning `turn.session.messages().to_vec()` so the frontend can render the full transcript on load.
- [ ] 2.7 **[MVP]** Implement `#[tauri::command] async fn clear_conversation(state)` that resets `turn.session = Session::new("desktop", "local")`. Document the persistence semantics: this clears the in-memory transcript only and does NOT remove the daily-note markdown or reset the learned `UserModel` that `Agent::process` already wrote to the data dir (per Devil's Advocate "what does clear mean" concern).
- [ ] 2.8 **[MVP]** Implement config commands that BYPASS `config::set()` (its allowlist silently no-ops `model.api_base`/`temperature`/`max_tokens` via a warn-only arm): `get_config() -> ConfigDto` (api_key masked in the DTO) and `save_config(model, api_key, api_base)` that `config::load()`, mutates the struct fields directly, then `config::save()`. Reload/rebuild the `Agent` in state if `model` changed.
- [ ] 2.9 **[MVP]** Register all commands in `tauri::Builder::default().invoke_handler(tauri::generate_handler![...])`. Confirm each is invocable from the devtools console via `window.__TAURI__.core.invoke`.
- [ ] 2.10 **[MVP]** Add a `tools_enabled: bool` flag (default `false`) to the managed `Turn` state and a `set_tools_enabled(enabled)` command. When `false`, skip `handle_tool_calls` dispatch in the desktop path (or gate it in agent) so the packaged app does NOT hand the LLM ungated `bash`/`write`/`edit` access by default. See 4.10 for the consent UI.
- [ ] 2.11 Note in code/comments that `call_llm` calls `config::load().await` on every turn (per-message disk I/O; runtime edits take effect mid-session). Acceptable for v1; flag a P1 task to cache `Config` in state and invalidate on `save_config`.

## Phase 3 — Frontend shell (OpenHumans-styled layout)

> Visual source of truth is the existing `src/ui/web.rs` `INDEX_HTML` (`:root` tokens: `--primary #2563eb`, `--primary-dark #1d4ed8`, `--accent #f59e0b`, `--success #10b981`, `--border #e2e8f0`, `--bg-secondary #f8fafc`, Inter font, card layout, 280px sidebar). Treat OpenHuman parity as "spirit, not pixel-match" — the target is an unverified secondary-source reconstruction.

- [ ] 3.1 **[MVP]** Port the `:root` CSS variables and base/reset styles from `web.rs` `INDEX_HTML` into the frontend stylesheet so the desktop matches the agreed tokens verbatim. Self-host the Inter font (bundle woff2 in `dist`) instead of the Google Fonts `<link>`, to satisfy the locked CSP (no external `font-src`).
- [ ] 3.2 **[MVP]** Build the app frame: a left **nav rail** (Chat, Settings for v1; Memory/Integrations/Skills as disabled/"coming soon" placeholders), and a main content area. Match the OpenHumans nav structure (Chat / Memory / Integrations / Skills / Settings) but only wire Chat + Settings in v1.
- [ ] 3.3 **[MVP]** Build the Chat view: a 280px session/info sidebar (model name, workspace, message count from `get_status`) + a scrollable message list + a bottom input bar with a send button. Port the message-bubble and card styling from `web.rs`.
- [ ] 3.4 **[MVP]** Add stable `data-testid` attributes from day one: `chat-input`, `send-button`, `message-list`, `message-bubble`, `settings-open`, `settings-api-key`, `settings-save`. (Required for the E2E plan in Phase 5.)
- [ ] 3.5 **[MVP]** Wire the chat send flow to `invoke('send_message', { message })`: optimistically render the user bubble, disable input while the request is in flight, append the returned assistant `Message`, re-enable. On `Err`, render a visible error bubble (uses the real HTTP error from 2.4).
- [ ] 3.6 **[MVP]** On app load, call `get_history` and `get_status` to hydrate the message list and sidebar.
- [ ] 3.7 **[MVP]** Add a 2-state "thinking" affordance only (idle vs. in-flight typing-dots indicator) tied to the send_message promise. Keep the existing static owl/emoji. The animated/voiced multi-state mascot is explicitly OUT of v1 (no asset pipeline, voice is stubbed) — leave a clearly-labeled stretch-goal stub.
- [ ] 3.8 Add minimal markdown + fenced-code rendering for assistant messages with a copy-code button (P1 polish; plain-text rendering is acceptable for MVP).

## Phase 4 — Features (MVP-first)

- [ ] 4.1 **[MVP]** Build the **Settings** view backed by `get_config`/`save_config` (2.8): fields for `model.model`, `model.api_base`, and a masked `model.api_key` input, with a Save button (`data-testid="settings-save"`). Show a success/error toast on save.
- [ ] 4.2 **[MVP]** Implement the **API-key gate**: on launch, if `config.model.api_key` is empty, route to Settings (with a "Add your API key to start chatting" banner) instead of the Chat view. This is the #1 first-run failure mode (default key ships blank).
- [ ] 4.3 **[MVP]** Add a `clear_conversation` button in the Chat sidebar wired to 2.7, with a confirm dialog clarifying it clears only the visible conversation.
- [ ] 4.4 Add a **Tools / permission** section in Settings exposing the `tools_enabled` toggle (2.10), defaulting OFF, with copy explaining the model would gain `bash`/file-write access on this machine.
- [ ] 4.5 **[Windows]** Confront the bash-tool reality: `src/tools/bash.rs` runs `Command::new("bash").arg("-c")` and `bash` is not guaranteed on Windows (the primary target). Add an availability check or route to PowerShell/cmd on Windows so an enabled `bash` tool doesn't hard-error on the demo machine. (P1 — only relevant once tools are enabled.)
- [ ] 4.6 **[P1]** Fix the tool-call parse regex `\[TOOL:(\w+):(\{.*?\})\]` (`src/core/agent.rs` `parse_tool_call`): the non-greedy `\{.*?\}` truncates at the first `}`, so any nested-JSON args (`write` content containing braces, `todo_write` arrays) silently fail to parse. Replace with brace-balanced extraction before surfacing tools in the desktop. Add a unit test with nested-object args.
- [ ] 4.7 **[P1]** Light/Dark theming: add a dark palette, a toggle persisted in config, and respect `prefers-color-scheme` on first run.
- [ ] 4.8 **[P1]** Conversation history persistence: persist `Session`s to their OWN SQLite schema (do NOT reuse/pollute `memory.db` FTS store) keyed by session id; left-rail list with title/timestamp + resume/delete.
- [ ] 4.9 **[P1]** LLM token streaming: refactor `call_llm` to send `stream: true` and consume reqwest's SSE byte stream; expose a `send_message_stream(channel: tauri::ipc::Channel<StreamEvent>, message)` command. Reuse `src/core/streaming.rs` `StreamEvent` as the wire schema (add `#[serde(rename_all = "camelCase")]`) so CLI NDJSON and desktop share one model. Render token-by-token with a stop button.
- [ ] 4.10 **[P1]** Per-tool confirm-before-run dialog: when `tools_enabled` is on, surface a Tauri-command-driven confirm prompt before each `bash`/`write`/`edit` runs (the existing `perm`/`plan_mode` handlers enforce nothing — they return placeholder text).
- [ ] 4.11 **[P1]** System tray via Tauri's built-in tray-icon API (Open / Quick Ask / Quit) — add the corresponding capability permission entry.
- [ ] 4.12 **[P1]** Global hotkey quick-launch via `tauri-plugin-global-shortcut` (default e.g. `Ctrl+Shift+Space`) toggling a small Raycast-style ask window — add its capability permission and budget per-OS testing.
- [ ] 4.13 **[P1]** Window-state persistence via `tauri-plugin-window-state` and single-instance via `tauri-plugin-single-instance` (register single-instance FIRST) — each with its capability entry.
- [ ] 4.14 **[P2]** Memory browser (read-first): surface `MemoryStore.search_fts`/`list_by_category` (SQLite/FTS5) and `MemoryWorkspace` (`MEMORY.md` + daily notes) in a searchable two-pane panel.
- [ ] 4.15 **[P2]** Integrations view (category cards + status pills + Connect/OAuth) — gate on MCP/channels actually being wired end-to-end (most of gateway is placeholder today).

## Phase 5 — Testing (Rust + E2E)

> E2E reality (Senior Dev verification): official Tauri 2 E2E is WebDriver via `tauri-driver` on **Windows + Linux only** — macOS has no WKWebView driver. Do not promise native macOS E2E. Commit to the split below.

- [ ] 5.1 **[MVP]** Add Rust `#[cfg(test)]` tests for the new command layer / config round-trip: `save_config` then `get_config` returns the written values; `get_config` masks the api_key. (Runs everywhere, CI-friendly.)
- [ ] 5.2 **[MVP]** Add a Rust test asserting `send_message` returns a `Message` (mock or feature-gate the LLM call) and that the turn `Mutex` serializes two concurrent calls without panicking / interleaving session writes.
- [ ] 5.3 **[MVP]** Add a Rust test for the 2.4 status-check: a non-2xx response from `call_llm` yields an `Err` (not an empty `Ok("")`).
- [ ] 5.4 **[P1]** Stand up Playwright against the **plain web frontend dev server in Chromium** for UI-logic smoke tests (app loads, key-gate routes to settings, send renders a reply, theme toggle, open settings) using the `data-testid` hooks from 3.4. This avoids the WebView2/WebDriver setup tax for fast UI iteration.
- [ ] 5.5 **[P1]** Add native smoke tests via `tauri-driver` + WebdriverIO (or Selenium), gated to **Windows + Linux CI only**: launch the bundled app, send one message, assert a reply renders. Document that macOS native E2E is intentionally out of scope.

## Phase 6 — Packaging / CI / Docs

- [ ] 6.1 **[MVP]** Configure `tauri.conf.json` `bundle` (identifier `com.openassistant.desktop`, productName, icons). Run `cargo tauri build` on Windows and confirm a runnable bundle/installer is produced.
- [ ] 6.2 **[MVP]** Add npm scripts (`dev`, `build`, `tauri dev`, `tauri build`) and document the desktop build/run steps in `CLAUDE.md` / README without breaking the existing `cargo` CLI build instructions.
- [ ] 6.3 **[MVP]** Add an explicit "What is NOT in v1" note shipped with the PR/build: streaming, history persistence, tray, global hotkey, memory browser, dark mode, mascot animation, voice, and all multi-agent / sub-agent / goal / plan-mode panels (those core handlers are stubs). Prevents advertising stubbed features as working.
- [ ] 6.4 **[P1]** Add a CI workflow that runs `cargo build` (root + src-tauri), `cargo clippy`, `cargo test`, and `cargo tauri build` on Windows + Linux; run the Playwright web-frontend suite (5.4) in Chromium; run `tauri-driver` native smoke (5.5) on Win/Linux only.
- [ ] 6.5 **[P1]** Verify the OS WebView variance on each target platform manually (WebView2 on Windows, WebKitGTK on Linux, WKWeb8 on macOS): chat send/receive, settings save, key-gate routing.
- [ ] 6.6 **[P2]** Wire `tauri-plugin-updater` for auto-update, with the corresponding capability permission and update-endpoint config.
- [ ] 6.7 **[MVP]** Final acceptance pass: on a clean machine with a valid API key, `cargo tauri build` → install → launch → app routes to Settings when key is empty → after saving key, Chat sends a real message and renders the actual LLM reply (not a stub), with errors surfaced visibly. Confirm `cargo build` + `cargo run -- tui|status|web` still pass (CLI not regressed by the lib refactor).
