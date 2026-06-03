## ADDED Requirements

### Requirement: WebChat runs the real agent

The WebChat messaging server SHALL process incoming messages through `Agent::process` and return the model's response, replacing the echo stub. [P0]

#### Scenario: A posted message is answered by the agent

- **WHEN** a client `POST`s `{"content": "..."}` to `/api/messages`
- **THEN** the server runs the content through `Agent::process` against a persisted web conversation and returns the assistant message
- **AND** the reply is the agent's output (or a surfaced error), never `"Echo: ..."`

#### Scenario: Turns do not interleave

- **WHEN** two posts arrive concurrently
- **THEN** the shared web conversation's lock serializes the turns so session writes cannot interleave

### Requirement: Telegram channel

The gateway SHALL connect to Telegram via the Bot API and answer allowed chats through the agent. [P1]

#### Scenario: A Telegram message is answered

- **WHEN** a user messages the bot and the token is valid
- **THEN** the long-poll loop routes the text through `Agent::process` for that chat and replies via `sendMessage`
- **AND** long replies are split under Telegram's length cap

#### Scenario: Invalid token fails fast

- **WHEN** the configured token is rejected by `getMe`
- **THEN** the Telegram task errors with a clear message rather than silently idling

### Requirement: Slack Events channel

The gateway SHALL accept Slack Events API callbacks on `POST /slack/events`, verifying request signatures, and answer message events through the agent. [P1]

#### Scenario: Signature verification

- **WHEN** a request arrives with a valid `X-Slack-Signature`/timestamp for the configured signing secret within the freshness window
- **THEN** it is accepted; otherwise it is rejected with 401

#### Scenario: URL verification handshake

- **WHEN** Slack sends a `url_verification` payload
- **THEN** the server echoes the `challenge` value with 200

#### Scenario: Message event is answered out-of-band

- **WHEN** a non-bot message event arrives
- **THEN** the server returns 200 immediately and, on a separate task, runs `Agent::process` for that channel and replies via `chat.postMessage`

### Requirement: Gateway orchestration

`start_gateway` SHALL run all configured channels, logging (not swallowing) channel task failures. [P1]

#### Scenario: Channels start together

- **WHEN** Discord and/or Telegram tokens are configured
- **THEN** each starts on its own task with errors logged, while the WebChat server (hosting the Slack route) runs in the foreground
