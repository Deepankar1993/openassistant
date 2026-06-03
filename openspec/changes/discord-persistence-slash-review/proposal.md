# Discord: Persistence, Slash Commands & Self-Review

## Why

Three follow-ups to the Hermes-style Discord bot were outstanding:
1. **Owned-thread memory was in-memory** — after a restart the bot forgot which threads it owned and lost each conversation's history.
2. **Text commands only** — no `/` slash commands.
3. **No periodic "self-improvement review"** post (the Hermes cron feature shown in the reference screenshot).

This change implements all three.

## What Changes

1. **SQLite persistence (`discord.db`).** A new `gateway::discord_store::DiscordStore` persists owned thread ids and each conversation's `Session` (as JSON, keyed by channel/thread id). On connect, the bot loads its owned-thread set, so **threads survive restarts** and continue with full history; every turn loads/saves that conversation's session (the store lock is never held across `Agent::process().await`).

2. **Slash commands.** Registered **per-guild** on connect (instant, unlike global): `/ask <message>` (deferred, then followups — handles the 3s interaction deadline + long replies), `/home`, `/unset_home`, `/new`, `/help`. An `interaction_create` handler gates by the allowlist and routes them. Text commands still work. Added the non-privileged `GUILDS` intent so `ready.guilds` is populated for registration.

3. **Self-improvement review (cron).** When `gateway.discord_review_hours > 0`, a background task posts a short reflection to the home channel every N hours and **appends it to the daily memory note** (so "memory updated" is literally true). It re-reads config each tick (home channel / interval can change without a restart) and degrades gracefully to "Memory updated." if the LLM call fails.

## Impact

**Affected spec:** extends `discord-gateway`.

**Affected / new code:**
- `src/gateway/discord_store.rs` — NEW: SQLite store (owned threads + sessions).
- `src/gateway/mod.rs` — `pub mod discord_store;`.
- `src/gateway/discord.rs` — store-backed sessions, owned-thread seeding + persistence, slash-command registration + `interaction_create`, `GUILDS` intent, `review_loop` + `generate_review`.
- `src/config/mod.rs` — `gateway.discord_review_hours` (+ `set` key); `discord_home_channel` set key (added earlier).
- `src/onboarding/wizard.rs` — init the new field.

**Operational notes:**
- Slash commands appear per-guild on connect; the bot must be invited with the `bot` scope (commands are registered via the bot token, no extra scope needed for guild commands).
- Self-review is **off by default**; enable with `config --key gateway.discord_review_hours --value 12`.

## Non-Goals

- **Global slash commands** — per-guild registration is used (instant); global (≤1h propagation) is not.
- **Desktop UI for review cadence** — set via CLI/config for now; the bot's `set home` / `/home` sets the home channel.
- **Per-message session rows** — the whole `Session` is stored as one JSON blob per conversation (simple, sufficient); a normalized message table is a later optimization.
