## ADDED Requirements

### Requirement: Discord bot integration

The gateway SHALL connect to Discord via `serenity` and reply to allowed users by running their message through `Agent::process`, replacing the no-op stub. [P2]

#### Scenario: An allowed user gets an agent reply

- **WHEN** an allowed user sends a message the bot can read
- **THEN** the bot runs it through `Agent::process` and replies with the result

#### Scenario: Bots and disallowed users are ignored

- **WHEN** the message author is a bot, or is not in `gateway.discord_allowed_users` (and the allowlist is non-empty), or is disallowed by `gateway.dm_policy`
- **THEN** the bot does not respond

#### Scenario: Message content intent is surfaced

- **WHEN** the Discord handler starts
- **THEN** it logs a one-time notice that the MESSAGE_CONTENT privileged intent must be enabled in the Developer Portal or message text will be empty

### Requirement: Per-user session isolation without lock-across-await

Each Discord user SHALL have an isolated conversation `Session`, stored behind an async mutex whose guard is never held across the `Agent::process` await, and bounded in size for a long-running process. [P2]

#### Scenario: Concurrent users are not serialized

- **WHEN** two allowed users message the bot simultaneously
- **THEN** the per-user session state is cloned/taken out and the lock guard is dropped before `Agent::process().await`, so one user's turn does not block the other's

#### Scenario: Sessions stay bounded

- **WHEN** a user has a long conversation
- **THEN** the in-memory session is trimmed so it does not grow unbounded

### Requirement: Gateway wiring and config keys

`start_gateway` SHALL start the Discord handler on a spawned task that logs errors, and `config::set` SHALL accept `gateway.discord_allowed_users` and `gateway.dm_policy`. [P2]

#### Scenario: Discord starts alongside WebChat

- **WHEN** a `gateway.discord_token` is configured and `gateway` is run
- **THEN** the Discord handler is spawned and a failure on that task is logged rather than silently lost

#### Scenario: Allowlist and policy are settable

- **WHEN** the user runs `config --key gateway.discord_allowed_users --value "123,456"` and `config --key gateway.dm_policy --value pairing`
- **THEN** both values are written to `config.yaml`
