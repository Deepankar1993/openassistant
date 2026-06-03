# Claude Code CLI Bridge

## Why

The user runs **Claude Code** (`claude`) locally and wants to drive it *through* openAssistant — especially from Discord — so a message in a thread continues a real Claude Code session working on a real project (the Hermes "bridge skill" idea, but deeper and integrated). openAssistant becomes a human-friendly front-end (Discord/CLI/web) for the same Claude the user already uses.

## What Changes

1. **`core/claude_bridge.rs` — `ClaudeBridge`.** Runs `claude -p --output-format json` non-interactively (prompt over stdin), parses the structured result (`result`, `session_id`, `is_error`, `total_cost_usd`). Builds args from config: `--model`, `--append-system-prompt`, `--permission-mode <mode>` (or `--dangerously-skip-permissions`), and `--resume <session-id>` for continuity. Runs in a configurable project directory with a per-call timeout. `available()` probes `claude --version`.

2. **Config `[claude]` section** (`ClaudeBridgeConfig`, all `#[serde(default)]`): `enabled`, `bin`, `workspace`, `model`, `permission_mode` (default `acceptEdits`), `skip_permissions`, `append_system_prompt`, `timeout_secs`, `discord_default`. All settable via `config::set`.

3. **`claude` agent tool.** `[TOOL:claude:{"prompt":"…","resume":"…"}]` delegates a task to Claude Code from any surface (CLI/web chat).

4. **Discord bridge mode (Hermes-style).** When `claude.enabled && claude.discord_default`, Discord conversations route through `ClaudeBridge` instead of the built-in agent. Each thread/DM maps to a **persistent Claude session id** stored in `discord.db` (`claude_sessions`), so a thread is one continuous Claude Code session. A **human, persona-flavored system prompt** is appended so replies feel like a warm teammate, not a task runner.

5. **`claude` CLI command.** `openassistant claude "<prompt>" [--resume <id>]` — one-shot bridge call (test harness + quick access), printing the result, session id, and cost.

## Impact

**Affected spec (new capability):** `claude-bridge`.

**Affected / new code:**
- `src/core/claude_bridge.rs` — NEW.
- `src/core/mod.rs` — `pub mod claude_bridge;`.
- `src/config/mod.rs` — `ClaudeBridgeConfig` + `set` keys.
- `src/core/agent.rs` — `claude` tool handler + `default_tools` entry.
- `src/gateway/discord_store.rs` — `claude_sessions` table + get/set.
- `src/gateway/discord.rs` — bridge field, `respond_via_claude`, persona/human prompt, build in `start`.
- `src/main.rs` — `claude` subcommand.
- `src/onboarding/wizard.rs` — init the new config field.

**Verified:** unit tests (arg building, JSON parse, resume, skip-permissions); **real end-to-end** (`openassistant claude` returned a live Claude reply, captured the session id + cost, and `--resume` recalled the prior turn); gateway started with **Claude bridge ON** and **Discord connected**.

## Non-Goals

- **Routing WebChat through Claude** — Discord is the bridge target; WebChat/CLI stay on the configured LLM (the `claude` tool is available on demand).
- **Streaming Claude output token-by-token to Discord** — replies are sent on completion (chunked).
- **Per-message tool-permission UI** — headless `claude` uses the configured `permission_mode`/`skip_permissions`; full autonomy (`bypassPermissions`) is opt-in.
- **Changing `claude.workspace` without restarting the gateway** — the gateway binds the bridge workspace at start; the `claude` tool/CLI picks up changes live.
