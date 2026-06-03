# Tasks

- [x] 1. Add `gateway.discord_home_channel` config field (`#[serde(default)]`) + `config::set` key; init in wizard.
- [x] 2. Rework `discord.rs`: mention/home → ✅ react + `create_thread_from_message` + reply in thread.
- [x] 3. Continue conversations inside bot-created threads (tracked thread-id set); DMs answered directly.
- [x] 4. Per-channel/thread sessions (lock dropped across `process().await`), bounded; seed home from config.
- [x] 5. Commands: `set home`/`!home`, `unset home`, `!new`/`!reset`, `!help`.
- [x] 6. ✅ reaction acknowledgement on handled messages.

## verification
- [x] 7. `cargo build --workspace` clean (serenity thread/reaction API compiles).
- [x] 8. `cargo test --lib gateway` passes (gate + thread_title + chunking).
