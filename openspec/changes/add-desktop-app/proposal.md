# Add Desktop Application

## Why

openAssistant's near-term goal (per `openspec/project.md`) is a cross-platform desktop app that gives the assistant a first-class native home, visually and behaviorally matching the OpenHumans desktop experience while reusing the existing Rust agent core. Today there is no desktop app and, critically, **no working UI at all**:

- The real agent loop exists and works: `Agent::process(&self, message, ctx, session) -> Result<String>` in `src/core/agent.rs` (lines 55-92) builds the system prompt, calls an OpenAI-compatible endpoint via `reqwest`, and does single-shot text-based tool dispatch.
- **Both shipped UIs are facades that never call it.** `src/ui/web.rs` `handle_chat` returns the literal string `"This is a simulated response. In production, this would run the full 11-step ReAct agent loop."` and `src/ui/tui.rs` `send_message` returns `"In a full implementation..."`. Neither imports `Agent`.

So the highest-value work is **wiring a real chat path into a native shell**, not re-skinning a placeholder. The danger is the inverse: a team could port the existing web UI's look, demo it, and ship a non-functional app that still returns the hardcoded "simulated response" string. This change exists to wire `Agent::process` directly into a Tauri 2.x shell that reuses the Rust core in-process.

Two hard prerequisites and one ground-truth correction force the design:

1. **The crate is binary-only.** `Cargo.toml` declares package `open-assistant` with no `[lib]` target; modules are declared with `mod` in `src/main.rs` (lines 2-13). A `src-tauri` crate cannot `use open_assistant::...` until a library target exists. This is a hard gate, not optional.
2. **The default API key ships empty** (`config/mod.rs`), and `call_llm` (lines 156-182) does `resp.json()` with no status check and an `unwrap_or("")` fallback — so the very first chat on a fresh install silently returns a blank assistant bubble instead of an error. First-run UX and error surfacing must be designed in, not bolted on.
3. **Tauri's bundled OS WebView (WebView2 / WebKitGTK / WKWebView) is an explicit, accepted exception** to the "single-binary ethos" constraint in `project.md`. The desktop shell pulls in the platform webview; the agent core remains a single Rust crate linked in-process.

## What Changes

This is the **MVP (v1)** scope. It is deliberately tight: the research digest's "P0" list (streaming, history persistence, tray, global hotkey, memory browser, dark mode, provider profiles, animated mascot) is reconciled here into P1/P2 per the expert panel. v1 ships a working, wired, native chat app — nothing more.

**In scope for v1:**

1. **Library/binary refactor (gating prerequisite, isolated first commit).** Add `[lib] name = "open_assistant" path = "src/lib.rs"` to `Cargo.toml` and create `src/lib.rs` re-exporting the modules `src-tauri` needs (`pub mod core; pub mod config; pub mod tools; pub mod memory; pub mod skills;`). Make `main.rs` consume the lib. The existing clap CLI binary stays intact. `cargo build` and `cargo run -- tui|status|web` must pass **before** any Tauri code is added, on its own branch — this touches every internal path and must be bisectable.

2. **A separate `src-tauri/` crate** (cargo-tauri 2.x scaffold) depending on `open-assistant = { path = ".." }` plus `tauri = { version = "2" }`. The root crate is NOT converted into the Tauri crate; the desktop app is added alongside the CLI.

3. **One wired, non-streaming `send_message` Tauri command.** An async `#[tauri::command]` that locks shared state, calls `Agent::process`, and returns the assistant `Message` (which is `Serialize`). Plus a minimal supporting surface: `get_status`, `get_history`, `clear_conversation`. Errors are mapped to `String` via `.map_err(|e| e.to_string())`.

4. **Combined managed state behind a single `tokio::sync::Mutex`.** A struct holding `{ agent, ctx, session }` (or `agent` outside, mutable `{ ctx, session }` under one lock). MUST be `tokio::sync::Mutex` (or `tauri::async_runtime::Mutex`), **not** `std::sync::Mutex`, because the guard is held across the `agent.process(...).await` and a `std` guard is `!Send` and will not compile in an async command. The single lock guards a whole conversational "turn" so double-Enter cannot interleave session writes or duplicate the daily-note/`observe` side effects.

5. **The Tauri frontend ports the existing `src/ui/web.rs` `INDEX_HTML`** (CSS variables `--primary #2563eb`, `--accent #f59e0b`, Inter font, card layout, 280px sidebar, message bubbles, typing-dots indicator) as the visual source of truth. OpenHumans/OpenHuman parity is treated as **spirit, not pixel-match** (the reference is an unverified secondary-source reconstruction of tinyhumansai's "OpenHuman"; the tokens we control are authoritative). A left nav rail (Chat / Memory / Integrations / Skills / Settings) may be stubbed structurally, but only **Chat** and **Settings** are functional in v1.

6. **A Settings/onboarding screen that can set the API key.** Wired to config via **direct `Config` struct mutation + `config::save()`**, NOT `config::set()` — `config::set()` (config/mod.rs lines 158-172) only handles `model.provider/model/api_key` + gateway tokens + `security.dm_pairing` and silently no-ops any other key via a `_ => warn!` arm, so saving `model.api_base`, `temperature`, or `max_tokens` through it appears to succeed but writes nothing. v1 exposes `model.api_key`, `model.model`, and `model.api_base` as plain fields (api_key masked). On launch, **if `config.model.api_key` is empty, route to Settings instead of Chat** — this is the #1 first-run failure mode.

7. **Surface real LLM errors.** Add an `resp.status().is_success()` check in `call_llm` before `resp.json()`, returning `anyhow::bail!` with status + body on failure. This ~5-line change converts the silent blank-bubble 401 into a visible, actionable error in the UI (and benefits the CLI). The frontend renders command errors as an error state, not an empty assistant message.

8. **Modern Tauri 2.x security posture, wired from day one:**
   - **Explicit CSP** in `tauri.conf.json` `app.security.csp` (Tauri 2 ships CSP **disabled** by default). Because the Rust backend makes the LLM HTTP call — not the webview — lock it down to roughly `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'` with **no `connect-src` to the LLM origin**. Do not add the OpenRouter host; the webview never needs outbound network.
   - **Default-deny capabilities.** A `src-tauri/capabilities/*.json` granting only `core:default`; per-plugin permissions are added only when their plugins land (P1+).

9. **A v1 tool-permission decision (security is a release blocker, not a footnote).** Exposing `send_message` hands an LLM steerable shell/file access on the user's machine; the existing `perm`/`plan_mode` handlers are placeholder text and enforce nothing — this gating must live in `agent.rs`/the tool layer, NOT in Tauri capabilities (the tools run inside the Rust core). For v1, ship **tools off-by-default** with an explicit opt-in, OR a one-time confirm-before-first-tool-run consent gate. Do not ship a packaged desktop binary with default-on, ungated `bash`/`write`.

10. **Confront the Windows `bash` reality.** `src/tools/bash.rs` unconditionally runs `Command::new("bash").arg("-c")` (line 55) — the "sandboxed" doc comment is inaccurate, and `bash` is not guaranteed on PATH on Windows 11, the primary target. v1 either gates `bash` behind an availability check (clean error if absent) or routes to PowerShell/cmd on Windows, so the flagship tool does not hard-error on the demo machine. (This pairs with item 9's off-by-default posture.)

11. **Testing harness that accounts for the macOS WebDriver gap.** Official Tauri 2 E2E is WebDriver via `tauri-driver` (WebdriverIO/Selenium) and works on **Windows + Linux only** — macOS has no WKWebView driver, so cross-platform native E2E parity is impossible. v1 commits to: (a) Rust `#[cfg(test)]` unit/integration tests for the new command layer + a config round-trip (runs everywhere, fast, CI-friendly) — the first tests in the repo; (b) optional Playwright/Chromium against the dev web frontend for UI logic; (c) `tauri-driver` native smoke tests gated to Windows + Linux CI only. **Stable `data-testid` attributes** on chat input, send button, message list, and settings fields are added from day one. Native macOS E2E is explicitly not promised.

**Explicitly NOT in v1** (per Industry Veteran "force the cut" + Devil's Advocate "true MVP"): token-by-token SSE streaming; conversation-history persistence to SQLite; system tray; global hotkey / quick-launch spotlight; memory browser; light/dark theming; provider-profile switching; the state-driven animated/voiced mascot (depends on a stubbed voice subsystem and is pure net-new animation — the single most likely item to sink the timeline); and any surfacing of the stubbed core features (`goal_deliberate`, `task`/sub-agent, `plan_mode`, multi-agent, teams, most of `gateway`) as working desktop functionality. These are documented as P1/P2 in `design.md`/`tasks.md`, with streaming specified (when it lands) as a `#[tauri::command]` taking `tauri::ipc::Channel<StreamEvent>` and reusing `src/core/streaming.rs` `StreamEvent` (add `#[serde(rename_all="camelCase")]`) as the shared wire schema.

## Impact

**Affected specs:**
- New capability: `desktop-app` (the Tauri shell, command bridge, settings/onboarding, security posture).

**Affected / new code:**
- `Cargo.toml` — add `[lib]` target (gating change).
- `src/lib.rs` — NEW: re-exports core modules for in-process reuse.
- `src/main.rs` — consume the lib instead of declaring `mod` (lines 2-13).
- `src/core/agent.rs` — `call_llm` gains an HTTP status check before `resp.json()` (lines 156-182); v1 tool-permission gating; optional token/cost capture from the API `usage` field (otherwise `get_status` must not surface fabricated zeros for `tokens_in/out`/`cost`).
- `src/tools/bash.rs` — Windows `bash`-availability handling / shell routing; correct the misleading "sandboxed" doc comment.
- `src/config/mod.rs` — settings writes use direct `Config` mutation + `save()`; document that `set()`'s allowlist does not cover `model.api_base`/`temperature`/`max_tokens`.
- `src-tauri/` — NEW crate: `tauri.conf.json` (with explicit CSP), `Cargo.toml`, `src/main.rs` (Builder `.setup` loads config, constructs `Agent::new(config.model.model).with_workspace(...)`, `.manage(state)`), `capabilities/*.json` (default-deny), and the command handlers.
- Frontend (`src-tauri/` dist or sibling) — NEW: HTML/CSS/JS ported from `src/ui/web.rs` `INDEX_HTML`, plus Settings/onboarding view and `data-testid` hooks.
- Tests — NEW: first `#[cfg(test)]` modules (command layer + config round-trip); E2E harness scaffolding (browser/Chromium + Windows/Linux `tauri-driver`).

**Behavioral / side-effect notes (must be reflected in UX copy):**
- `Agent::process` appends to `memory/YYYY-MM-DD.md` daily notes and mutates the `UserModel` on every message. `clear_conversation` resets the in-memory `Session` only; it does NOT wipe daily notes or reset the learned user model. v1 sessions are **in-memory and lost on restart** (acceptable, explicit decision — no new SQLite history schema, which would pollute the FTS memory store).
- `call_llm` reloads config from disk every turn (acceptable for v1; means settings edits take effect mid-session).

**Docs:** Note in project docs that the OS WebView is the sanctioned exception to the single-binary constraint, and that `ARCHITECTURE.md`'s workspace/`crates/` design remains aspirational.

## Non-Goals

- **Forking or rewriting the agent loop.** The desktop app calls `Agent::process` as-is; it does not duplicate or branch the loop (per `project.md` constraint).
- **Streaming chat UX in v1.** Non-streaming `send_message` returning the final `Message` is what `Agent::process` already provides; a typing-dots indicator while the request is in flight is sufficient. SSE/Channel streaming is a specified P1 follow-up, treated as net-new async plumbing.
- **Conversation history persistence, tray, global hotkey, memory browser, theming, and provider profiles.** All P1/P2; not built in v1.
- **The animated/voiced mascot.** Drawn entirely from unverified secondary sources and dependent on a stubbed voice subsystem; the static owl with at most a 2-state idle/thinking CSS treatment is the v1 ceiling. Explicitly a stretch goal.
- **Surfacing stubbed core features.** `goal_deliberate`, `task`/sub-agent, `plan_mode`, `perm`, multi-agent, agent teams, and the Discord/Telegram/Slack gateways return placeholder text or are unwired; they are not exposed as working desktop functionality.
- **Pixel-perfect OpenHumans/OpenHuman cloning.** The reference is an inferred reconstruction of a third-party product; the openAssistant design tokens are the source of truth and parity is "spirit, not pixel-match."
- **Native macOS automated E2E.** No WKWebView WebDriver exists; macOS is covered by Rust unit tests and (optional) browser-based frontend tests only.
- **OS keychain / encrypted secret storage.** v1 keeps the api_key in `config.yaml` (masked in the UI, never logged); keychain integration is later work.
