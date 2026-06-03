# Gateway Config & Readiness (terminal + desktop)

## Why

The gateway now runs for real, but two gaps remained:

1. **No setup guidance.** A user with a Discord token but no allowlist (and `dm_policy` ≠ `open`) gets a silent bot — the gate ignores everyone — with nothing telling them why. There was no single place, on either surface, that listed what's required to run the gateway and what's missing.
2. **The WebChat server's address was not configurable.** The bind was hardcoded `0.0.0.0:{port}` and `webhook_port` wasn't even settable via `config::set`. There was no host/IP control, and the desktop Settings → Channels panel still showed the stale callout *"Gateway channels are experimental — the messaging server is not yet fully operational"* (`frontend/index.html`).

This change adds a **shared readiness report** surfaced on both the terminal and the desktop, and makes the WebChat **host/IP + port editable from both**, seamlessly.

## What Changes

1. **Shared readiness (one source of truth).** New `gateway::readiness(&Config) -> Vec<GatewayRequirement>` (pure, no I/O) reports each item — API key (required), WebChat bind address, Discord (incl. the "no allowlist ⇒ ignores everyone" warning + MESSAGE CONTENT reminder), Telegram, Slack (needs token + signing secret + public URL), and a **"How to run"** row that includes the `cargo run -- gateway` / built-binary fallback **for when `openassistant` isn't on PATH**. `format_readiness` renders it for the terminal.

2. **Terminal surface.** `openassistant gateway` now prints the readiness report before starting; `openassistant gateway --check` prints it and exits.

3. **Configurable WebChat address.** New `gateway.webhook_host` config field (`#[serde(default)]`; empty ⇒ `0.0.0.0`). `config::set` now accepts `gateway.webhook_host` and `gateway.webhook_port`. Shared resolvers `config::webchat_host` / `config::webchat_port` (port 0 ⇒ 3000) are used by both the server bind and the readiness report.

4. **Desktop surface.** Settings → Channels is rebuilt: the stale "experimental" callout is replaced by a **Setup-requirements panel** (a `gateway_readiness` Tauri command rendering the shared report), **Host/IP + Port inputs**, **Discord allowed-user IDs** + **DM policy** select, and a copyable start command with the not-on-PATH hint. `ConfigDto`/`FullConfigDto` carry the new fields; `save_full_config` persists them via load→mutate→save.

## Impact

**Affected spec (new capability):** `gateway-readiness`.

**Affected / new code:**
- `src/config/mod.rs` — `gateway.webhook_host`; `set` keys for host/port; `webchat_host`/`webchat_port` helpers.
- `src/gateway/mod.rs` — `GatewayRequirement`, `readiness`, `format_readiness`.
- `src/gateway/webchat.rs` — bind to resolved host:port.
- `src/main.rs` — `Gateway { check }`; print readiness.
- `src/onboarding/wizard.rs` — init the new field.
- `src-tauri/src/commands/system.rs` — `gateway_readiness` command.
- `src-tauri/src/commands/settings.rs` — DTO + persistence for host/port/allowlist/dm_policy.
- `src-tauri/src/lib.rs` — register `gateway_readiness`.
- `frontend/index.html`, `frontend/app.js` — Channels panel rebuild + readiness render + mock.
- `tests/e2e/*` — updated Channels test + mock.

## Non-Goals

- **Editing the Slack signing secret from the desktop** — it stays a `config.yaml`/CLI field (the readiness panel flags when it's missing).
- **A "Start gateway" button in the desktop app** — the gateway is a long-running server; the desktop surfaces the exact run command instead of hosting the server in-process.
- **Telegram allowlisting** — still no per-user gate for Telegram (a follow-up `gateway.telegram_allowed_users`).
