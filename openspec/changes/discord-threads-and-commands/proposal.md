# Discord: Hermes-style Threads & Commands

## Why

The Discord bot replied inline in the channel for every allowed message — noisy, and it loses the per-conversation structure. The user wants the **Hermes bot** behavior: @mention (or post in a "home" channel) → the bot reacts ✅ and **spawns a thread**, then the conversation continues **inside that thread**. Plus a `set home` command, as seen in the reference screenshot.

## What Changes

`gateway/discord.rs` is reworked from "reply to everything" to addressed, thread-based conversations:

1. **Trigger rules.**
   - **@mention** the bot in a channel, **or** post a top-level message in the **home** channel → react ✅, create a thread from that message (`create_thread_from_message`), and reply **inside the thread**.
   - A message **inside a bot-created thread** continues that thread's conversation — no mention needed.
   - **DMs** are answered directly (no threads in DMs).
   - Anything else in a guild channel is ignored (no more noise).

2. **Per-conversation sessions.** Sessions are keyed by **channel/thread id** (a thread = one conversation), not by user. The lock is dropped before `Agent::process().await`. Sessions are bounded; the set of bot-owned thread ids is tracked in memory (cleared on restart — re-mention to resume; SQLite-backed thread memory is a follow-up).

3. **Commands** (allowed users only): `set home` / `!home` (persist this channel as home in `gateway.discord_home_channel`), `unset home`, `!new` / `!reset` (fresh conversation here), `!help`.

4. **Config + acknowledgement.** New `gateway.discord_home_channel` (`#[serde(default)]`, settable via `config::set`), seeded on start. The bot adds a ✅ reaction to acknowledge each handled message.

## Impact

**Affected spec:** extends `discord-gateway`.

**Affected / new code:**
- `src/gateway/discord.rs` — thread/mention/home routing, ✅ reaction, per-thread sessions, commands.
- `src/config/mod.rs` — `gateway.discord_home_channel` + `set` key.
- `src/onboarding/wizard.rs` — init the new field.

**Operational note (permissions):** the bot now needs **Create Public Threads**, **Send Messages in Threads**, and **Add Reactions** in addition to View/Send/Read-History. Re-invite with those scopes (or Administrator).

## Non-Goals

- **Persistent thread memory across restarts** — the owned-thread set is in-memory for MVP (Hermes uses a DB); re-mention resumes. SQLite-backed thread/session storage is a follow-up.
- **Slash commands** — text commands for now (no `applications.commands` registration).
- **Periodic "self-improvement review" posts** — the Hermes cron-style status post is out of scope.
