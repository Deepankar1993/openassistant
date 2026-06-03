# Desktop Gateway Start/Stop

## Why

The CLI can start the gateway (`openassistant gateway`), and the desktop could *configure* it and show the run command — but it couldn't actually **start** it. The user asked for parity: both the CLI and the app should have a way to start the messaging server. Since the gateway is now fully real (all channels run the agent), an in-app Start/Stop control is honest.

## What Changes

1. **Core: an abortable run handle.** Refactor `gateway::start_gateway` to share a `run_all(config)` helper, and add `gateway::start_gateway_handle(config) -> GatewayRunHandle`. It **pre-binds** the WebChat address so a port conflict is reported to the caller (instead of vanishing into a spawned task), then runs the gateway on a background task. `GatewayRunHandle` exposes `stop()`, `is_running()`, `wait()` and the bound `addr`.

2. **Desktop commands.** `gateway_start`, `gateway_stop`, `gateway_status` (new `commands/gateway.rs`), backed by an `Option<GatewayRunHandle>` held in `AppCore`. Start refuses if already running or the port is busy and returns the listening URL.

3. **Desktop UI.** Settings → Channels gains a **Start gateway / Stop** control with a live status line (`● Running · http://…` / `○ Stopped`), alongside the existing copyable terminal command for users who prefer the CLI.

## Impact

**Affected spec:** extends `gateway-readiness` (see that change) with start/stop scenarios.

**Affected / new code:**
- `src/gateway/mod.rs` — `run_all`, `start_gateway_handle`, `GatewayRunHandle`.
- `src-tauri/src/state.rs` — `gateway: Mutex<Option<GatewayRunHandle>>`.
- `src-tauri/src/commands/gateway.rs` — NEW: start/stop/status.
- `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` — register.
- `frontend/index.html`, `frontend/app.js` — Start/Stop controls + status + mocks.
- `tests/e2e/*` — Channels test asserts start→running→stop→stopped.

## Non-Goals

- **Auto-starting the gateway on app launch** — it stays an explicit user action.
- **Surviving app close** — the in-process gateway stops when the desktop app exits; for an always-on server, run the CLI `openassistant gateway`.
- **Console-window changes** — already handled: `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` keeps release builds console-free; the dev console is `cargo tauri dev` only.
