# Complete Gateway Channels

## Why

After `complete-core-features-and-integrations` made the Discord channel real, the rest of the gateway was still a facade — the status line "Gateway channels are experimental; the messaging server is not yet fully operational" was accurate:

- **WebChat** (`src/gateway/webchat.rs:76`) — the HTTP "messaging server" — replied with `format!("Echo: {}", payload.content)`. It never constructed or called `Agent`.
- **Telegram** (`src/gateway/telegram.rs:6`) — `info!("Telegram gateway would start here")`, then `Ok(())`.
- **Slack** (`src/gateway/slack.rs:6`) — same no-op stub.

So `openassistant gateway` started a server that couldn't actually talk to the model on any channel except Discord. This change wires every channel through the real `Agent::process` loop so the gateway is genuinely operational.

## What Changes

1. **WebChat runs the real agent loop.** `send_message` now calls `Agent::process` and returns the model's reply instead of an echo. Shared `GatewayState { agent, config, web, slack_sessions }` holds one `Arc<Agent>`; the web UI shares a single `Convo { ctx, session, messages }` behind a `tokio::sync::Mutex` held for the whole turn (mirrors the desktop's per-turn lock so concurrent posts can't interleave session writes). `start` now takes the full `Config`, builds the agent from it, and defaults `webhook_port` to 3000 when unset (0).

2. **Telegram via Bot API long polling.** `telegram::start(Config)` validates the token with `getMe`, then loops on `getUpdates` (30s long poll, advancing `offset`), routing each chat's text through a per-chat `Agent::process` and replying with `sendMessage` (chunked to Telegram's 4096-char cap). Sessions are bounded. No new crate — plain `reqwest`.

3. **Slack via the Events API.** A `POST /slack/events` route is mounted on the WebChat axum server (only when Slack is configured). It verifies the `X-Slack-Signature` HMAC-SHA256 (with a 5-minute timestamp freshness window), answers the `url_verification` challenge, and for message events spawns an out-of-band task (to ack within Slack's 3-second deadline) that runs `Agent::process` per channel and replies via `chat.postMessage`. Bot messages and message subtypes are ignored.

4. **Gateway orchestration.** `start_gateway` spawns Discord and Telegram on their own tasks (errors logged, not swallowed), logs the Slack endpoint when configured, and runs the WebChat server (which hosts Slack) in the foreground.

5. **One new dependency.** `hmac = "0.12"` for Slack signature verification (`sha2`/`hex` already present).

## Impact

**Affected spec (new capability):** `gateway`.

**Affected / new code:**
- `src/gateway/webchat.rs` — real agent loop; `GatewayState`/`Convo`; `build_router`/`build_state`; `start(Config)`.
- `src/gateway/telegram.rs` — real long-poll bot.
- `src/gateway/slack.rs` — Events API handler, signature verification, `chat.postMessage`.
- `src/gateway/mod.rs` — spawn Telegram, note Slack, run WebChat server.
- `Cargo.toml` — add `hmac = "0.12"`.

**Operational notes:**
- Slack requires the WebChat server to be publicly reachable (tunnel/reverse proxy) and `gateway.slack_token` + `gateway.slack_signing_secret` set.
- Telegram and WebChat work with no public URL.
- WebChat uses a single shared conversation (no per-user auth on the local HTTP API); Discord/Telegram/Slack isolate sessions per user/chat/channel.

## Non-Goals

- **Authentication / multi-user sessions for the WebChat HTTP API** — it remains a single local conversation; the DM-pairing/allowlist security model applies to the chat platforms, not the raw HTTP endpoint.
- **The standalone `web` command UI** (`src/ui/web.rs`) — that is a separate surface (still a demo facade) and is out of scope here; this change is about the `gateway` messaging server.
- **Slack Socket Mode** — the Events API (webhook) path is implemented; Socket Mode (websocket) is not.
- **Telegram allowlisting** — no allowlist config field exists for Telegram yet; any chat may message the bot. A `gateway.telegram_allowed_users` key is a reasonable follow-up.
