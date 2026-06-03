# Tasks

## persistence
- [x] 1. New `gateway/discord_store.rs`: `DiscordStore` over `discord.db` (owned_threads + conversations); `pub mod` in gateway.
- [x] 2. Seed owned-thread set from the store on connect; persist new threads via `mark_thread`.
- [x] 3. Load/save each conversation's `Session` per turn (lock not held across `process().await`); `!new` clears it.

## slash commands
- [x] 4. `slash_commands()` (`/ask`, `/home`, `/unset_home`, `/new`, `/help`); register per-guild in `ready`.
- [x] 5. `interaction_create` handler: allowlist gate, `/ask` defer+followup, others ephemeral; add `GUILDS` intent.

## self-improvement review (cron)
- [x] 6. `gateway.discord_review_hours` config (+ `set` key); init in wizard.
- [x] 7. `review_loop` spawned when > 0 (re-reads config each tick); `generate_review` posts to home + appends to memory.

## verification
- [x] 8. `cargo build --workspace` clean (serenity interaction/thread API compiles).
- [x] 9. `cargo test --lib gateway` passes 7/7 (incl. `discord_store` round-trip + slash-commands present).
