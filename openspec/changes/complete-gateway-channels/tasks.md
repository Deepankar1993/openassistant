# Tasks

## webchat (the messaging server)
- [x] 1. Add `GatewayState { agent, config, web, slack_sessions }` + `Convo { ctx, session, messages }`.
- [x] 2. Rewrite `send_message` to call `Agent::process` and return the real reply (no more "Echo:").
- [x] 3. `build_state(Config)` + `build_router` (mounts `/slack/events` when Slack is configured); `start(Config)` defaults port to 3000.

## telegram
- [x] 4. `telegram::start(Config)` — `getMe` validation, `getUpdates` long-poll loop with offset.
- [x] 5. Per-chat sessions through `Agent::process`; reply via `sendMessage` (chunked); bound session growth.

## slack
- [x] 6. `events_handler` — HMAC-SHA256 signature verification with timestamp freshness window.
- [x] 7. `url_verification` challenge handshake.
- [x] 8. Message events → out-of-band `Agent::process` per channel → `chat.postMessage`; ignore bot/subtype messages.

## orchestration & deps
- [x] 9. `start_gateway` spawns Telegram (errors logged), logs Slack endpoint, runs WebChat foreground.
- [x] 10. Add `hmac = "0.12"`.

## verification
- [x] 11. `cargo build` clean.
- [x] 12. `cargo test --lib` — new gateway tests pass (Slack signature roundtrip, Discord gating/chunking); 2 pre-existing `permissions.rs` failures unrelated.
- [x] 13. Smoke test: `POST /api/messages` returns a real agent reply, not an echo.
