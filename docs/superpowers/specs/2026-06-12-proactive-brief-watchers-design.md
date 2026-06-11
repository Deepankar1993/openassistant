# Proactive Daily Brief + URL Watchers — Design

Date: 2026-06-12
Status: Approved (continuation of the 2026-06-10 viral-features research; user said "go ahead")

## Context

Market research identified the proactive loop — the assistant messaging you first —
as the top remaining viral bet (the OpenClaw pattern: people screenshot the morning
brief their agent sent to Discord). The Discord self-review cron
(`review_loop`/`generate_review`, src/gateway/discord.rs:428-496) already proves the
shape: spawned tokio loop → re-read config per tick → LLM call → post to home channel.
This batch generalizes that into a Daily Brief plus user-managed URL watchers.

## Features

### 1. Daily Brief (`src/core/brief.rs`)

- `generate_brief(cfg: &Config) -> Result<String>` — one LLM call composing a short
  morning brief from: persona name, MEMORY.md (truncated 2000), yesterday's + today's
  daily notes (truncated 1500 each), the goal board (`GoalStore` → `format_board()`),
  and any watcher changes in the last 24h. Persona-toned, ≤ 200 words, ends by
  inviting a reply. Appends "Brief delivered" to the daily note.
- Pure helper `brief_due(cfg_time: &str, last_sent_date: &str, now_local: &chrono::DateTime<Local>) -> bool`
  — true when local HH:MM ≥ configured time and the brief hasn't been sent today
  (unit-testable; no clock reads inside).
- New CLI command `openassistant brief` — prints the brief to stdout (demo-able
  without any channel configured).

### 2. URL Watchers (`src/core/watchers.rs`)

- `Watcher { id, url, note, interval_minutes, last_hash, last_checked, last_changed }`.
- `WatcherStore` — JSON persistence at `<data_dir>/proactive.json` (atomic write via
  tempfile, the goal-store pattern). The file also carries `last_brief_date` (shared
  proactive state, one file).
- `content_hash(text)` — sha2 over whitespace-collapsed body, so formatting churn
  doesn't false-positive. `check_due(client, now)` fetches each due watcher
  (`reqwest`, 15s timeout, body capped 512 KiB), compares hashes, updates state,
  returns the changed watchers.
- Change notification text: "🔭 {url} changed" + an LLM one-liner summary of the new
  content (truncated 3000 chars input); LLM failure degrades to the plain notice.
- New `watch` agent tool: `{"action":"add|list|remove","url":"…","note":"…","interval_minutes":60}`
  (default 60, min 5) so "watch this page for me" works from any channel. Wired into
  `execute_tool`, `default_tools()`, and treated as a normal non-read tool by the
  permission modes (Ask under Default, allowed under AcceptEdits like other
  write-ish tools — add to `accept_edits_check`/`auto_classify` allowlists).

### 3. Proactive gateway loop (`src/gateway/proactive.rs`)

- `proactive_loop(initial: Config)` — 60s `tokio::time::interval`; each tick re-reads
  config (dynamic enable/disable, same as `review_loop`), then:
  1. If `cfg.brief.enabled` and `brief_due(...)` → `generate_brief` → post →
     persist `last_brief_date`.
  2. `WatcherStore::check_due(...)` → for each change → post notification.
- Posting (`post_everywhere(cfg, text)`):
  - Discord: standalone `serenity::http::Http::new(&token)` +
    `ChannelId::new(home).say(...)`, chunked ≤ 1900 chars — no coupling to the
    serenity event client.
  - Telegram: existing `sendMessage` JSON shape to `cfg.brief.telegram_chat_id`
    (explicit config in v1; persisted chat discovery stays on the backlog).
  - No channel configured → log and skip (the `brief` CLI still works).
- Spawned unconditionally from `gateway::run_all` (cheap tick; flags honored live).

### 4. Config `[brief]` (serde-defaulted, additive)

`enabled: bool=false`, `time: "08:00"`, `discord: bool=true` (post to home channel),
`telegram_chat_id: String=""`. Settable keys: `brief.enabled`, `brief.time`,
`brief.telegram_chat_id`, `brief.discord`.

## Error handling

Loop ticks never crash the gateway: every step logs-and-continues. Watcher fetch
errors don't update `last_hash` (no false "change" after an outage; retried next
interval). LLM failures fall back to plain-text notices.

## Testing (offline)

- `brief_due` boundary cases (before/at/after time, already-sent today, bad config
  time falls back to 08:00).
- `WatcherStore` add/list/remove + JSON round-trip in a temp dir; `content_hash`
  whitespace insensitivity; due-selection by interval.
- `watch` tool dispatch through `execute_tool` (temp workspace).
- `[brief]` config defaults + round-trip; legacy YAML loads.
- Full `cargo test --workspace` stays green.
