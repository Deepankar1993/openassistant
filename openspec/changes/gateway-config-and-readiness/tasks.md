# Tasks

## core
- [x] 1. Add `gateway.webhook_host` (`#[serde(default)]`) to `GatewayConfig`.
- [x] 2. `config::set` accepts `gateway.webhook_host` and `gateway.webhook_port` (validated u16).
- [x] 3. Add `config::webchat_host` / `config::webchat_port` resolvers (empty ⇒ 0.0.0.0, 0 ⇒ 3000).
- [x] 4. Add `GatewayRequirement`, `gateway::readiness(&Config)`, `gateway::format_readiness`.
- [x] 5. `webchat::start` binds to the resolved host:port.

## terminal
- [x] 6. `Commands::Gateway { check }`; print readiness before start; `--check` prints and exits.

## desktop
- [x] 7. `gateway_readiness` Tauri command (system.rs) returning the shared report; register in lib.rs.
- [x] 8. `ConfigDto`/`FullConfigDto` carry webhook_host/port + discord_allowed_users + dm_policy; `save_full_config` persists them.
- [x] 9. Rebuild Settings → Channels: remove the stale "experimental" callout; add readiness panel, Host/Port inputs, allowed users, DM policy, copyable run command + not-on-PATH hint.
- [x] 10. `app.js`: load/save the new fields, render readiness, copy command; extend default + Playwright mocks.

## verification
- [x] 11. `cargo build --workspace` clean.
- [x] 12. `cargo test -p openassistant-desktop` passes (5/5); root lib new tests pass (2 pre-existing permissions failures unrelated).
- [x] 13. CLI smoke: `gateway --check` renders readiness; host/port round-trips via `config --key gateway.webhook_host/port`.
- [x] 14. Playwright E2E: full suite passes (54/54), incl. updated Channels gateway-config + readiness test.
