# Tasks

## bridge core
- [x] 1. `core/claude_bridge.rs`: `ClaudeBridge` (`from_config`, `available`, `build_args`, `run`) + `parse_result`; `pub mod` in core.
- [x] 2. Config `[claude]` section (`ClaudeBridgeConfig`, serde defaults) + `config::set` keys; init in wizard.

## integration
- [x] 3. `claude` agent tool (handler + `default_tools`).
- [x] 4. `discord_store`: `claude_sessions` table + get/set (cleared on `!new`).
- [x] 5. Discord bridge mode: `respond_via_claude` with per-conversation session resume; persona + human-tone system prompt; built in `start` when enabled.
- [x] 6. `openassistant claude "<prompt>" [--resume]` CLI command.

## verification & run
- [x] 7. `cargo build --workspace` clean.
- [x] 8. `cargo test --lib claude_bridge` 5/5; `cargo test --lib gateway` 7/7.
- [x] 9. Real end-to-end: `openassistant claude` returns a live reply + session id + cost; `--resume` recalls prior turn.
- [x] 10. Enable bridge (`claude.enabled=true`) and start the gateway — logs show "Claude bridge ON" + "Discord connected".
