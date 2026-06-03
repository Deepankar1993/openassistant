# Tasks

- [x] 1. Refactor `gateway::start_gateway` to share `run_all(config)`.
- [x] 2. Add `GatewayRunHandle` (stop/is_running/wait/addr) + `start_gateway_handle(config)` with pre-bind port-conflict detection.
- [x] 3. Add `gateway: Mutex<Option<GatewayRunHandle>>` to `AppCore`.
- [x] 4. New `commands/gateway.rs`: `gateway_start` / `gateway_stop` / `gateway_status`; register in `commands/mod.rs` + `lib.rs`.
- [x] 5. Frontend: Start/Stop buttons + live run-status in Settings → Channels; default + Playwright mocks for the three commands.
- [x] 6. Confirm console-window handling (windows_subsystem in release; log plugin debug-only) — no change needed.

## verification
- [x] 7. `cargo build --workspace` clean.
- [x] 8. `cargo test -p openassistant-desktop` 5/5.
- [x] 9. Playwright E2E 54/54, incl. Channels start→running→stop→stopped.
