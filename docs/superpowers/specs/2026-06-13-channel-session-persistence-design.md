# Telegram/Slack Session Persistence — Design

Date: 2026-06-13
Status: Approved (next backlog item; user: "start Telegram/Slack session persistence")

## Context

Discord persists conversations across restarts (`discord_store.rs`), but Telegram
(`HashMap<i64, Convo>` in the poll loop) and Slack (`GatewayState.slack_sessions:
Arc<Mutex<HashMap<String, Convo>>>`) keep sessions only in memory — a restart drops
every ongoing conversation's context. Both key by a stable external id (Telegram
`chat_id`, Slack `channel`), the same shape `discord_store` persists. So one small
generic store serves both.

## Architecture

### `ChannelSessionStore` (new, `src/gateway/session_store.rs`)

SQLite, the `discord_store` pattern (WAL, JSON `Session` blob, opened per-operation).
DB at `<data_dir>/gateway_sessions.db`. One table keyed by the (channel kind,
external id) pair so Telegram and Slack share it without collision:

```sql
CREATE TABLE IF NOT EXISTS channel_sessions (
    channel      TEXT NOT NULL,   -- "telegram" | "slack"
    external_id  TEXT NOT NULL,   -- chat_id / channel id
    session_json TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    PRIMARY KEY (channel, external_id)
);
```

API (`anyhow::Result`):
- `open_default(data_dir) -> Self` / `open(path) -> Self`
- `load(channel, external_id) -> Option<Session>`
- `save(channel, external_id, &Session)` — `INSERT OR REPLACE`, `updated_at = now`.

Unit-tested: round-trip, channel/id isolation (same external id under different
channels are distinct rows), missing → None.

### Telegram (`src/gateway/telegram.rs`)

Open the store once before the poll loop (single-tasked, so it owns one `Connection`).
On a chat's first message after (re)start, populate the in-memory `Convo` from
`store.load("telegram", chat_id)` instead of always `Session::new`. After each turn
(post-truncation, so the persisted blob stays bounded by `MAX_SESSION_MESSAGES`),
`store.save("telegram", chat_id, &convo.session)`. Save failures are logged, never
fatal.

### Slack (`src/gateway/slack.rs`)

`handle_message` runs per-event (possibly concurrent), so open the store per call
(matches `discord_store` usage). On a cache miss in `slack_sessions`, load from the
store before falling back to `Convo::new`. After `process`, bound the session to
`MAX_SESSION_MESSAGES` (new — parity with Telegram, prevents unbounded blob growth),
`store.save("slack", channel, &convo.session)`, then re-insert into the in-memory map
(write-through cache). The pre-existing same-channel concurrency race
(remove→process→reinsert) is unchanged — out of scope.

## Non-goals (deferred)

Proactive Telegram broadcast to all known chats (a `list_external_ids` discovery on
top of this store) is intentionally NOT wired here — auto-messaging every chat that
ever pinged the bot is a product decision; the explicit `brief.telegram_chat_id` stays
the delivery target.

## Error handling

A failed `load` falls back to a fresh session; a failed `save` logs and continues.
Persistence never breaks a turn.

## Testing (offline)

- `ChannelSessionStore`: save/load/round-trip, channel isolation, missing→None, in a
  temp dir. Reuse `Session` + `Message::user` like the `discord_store` test.
- `cargo test --workspace` green; existing Slack signature test stays green; clippy no
  new warnings. (The poll/event loops themselves aren't unit-tested — no network mock
  layer — consistent with the rest of the gateway.)
