## ADDED Requirements

### Requirement: Gateway readiness report

The system SHALL provide a single readiness report describing what is required to run the gateway and what is missing, surfaced identically on the terminal and the desktop. It SHALL include the WebChat bind address and a "how to run" hint covering the case where `openassistant` is not on the PATH. [P0]

#### Scenario: Terminal readiness check

- **WHEN** the user runs `openassistant gateway --check`
- **THEN** a readiness report is printed (API key, WebChat address, Discord/Telegram/Slack status, how-to-run) and the process exits without starting servers
- **AND** running `openassistant gateway` without `--check` prints the same report and then starts the servers

#### Scenario: Discord misconfiguration is explained

- **WHEN** a Discord token is set but the allowlist is empty and `dm_policy` is not `open`
- **THEN** the report marks Discord not-ready and states the bot will ignore everyone, with the exact keys to fix it

#### Scenario: Not-on-PATH guidance is present

- **WHEN** the readiness report is shown on either surface
- **THEN** it includes how to launch the gateway when `openassistant` is not on the PATH (e.g. `cargo run -- gateway` or the built binary)

#### Scenario: Desktop readiness panel

- **WHEN** the user opens Settings → Channels in the desktop app and clicks "Check requirements"
- **THEN** the same readiness items render in the panel

### Requirement: Configurable WebChat address

The WebChat/Slack server bind host and port SHALL be configurable from both the terminal and the desktop, with sensible defaults (host `0.0.0.0`, port `3000`). [P0]

#### Scenario: Set host and port from the terminal

- **WHEN** the user runs `config --key gateway.webhook_host --value 127.0.0.1` and `config --key gateway.webhook_port --value 8080`
- **THEN** both persist, and the readiness report and server bind reflect `http://127.0.0.1:8080`

#### Scenario: Set host and port from the desktop

- **WHEN** the user edits Host/IP and Port in Settings → Channels and saves
- **THEN** the values persist via the full-config save path (load → mutate → save)

#### Scenario: Defaults when unset

- **WHEN** `webhook_host` is empty and `webhook_port` is 0
- **THEN** the server binds `0.0.0.0:3000`
