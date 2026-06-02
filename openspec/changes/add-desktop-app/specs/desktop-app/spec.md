# Desktop App

The openAssistant cross-platform desktop application: a Tauri 2.x shell that reuses the existing Rust agent core (`open_assistant` library) in-process and presents the OpenHumans-inspired card-based aesthetic (Inter, primary blue `#2563eb`, amber accent `#f59e0b`, light theme with `#f8fafc` background and `#e2e8f0` borders).

This spec is **scoped tightly**. Each requirement is tagged with a priority derived from the expert panel:

- **[P0 / MVP]** — required for the first shippable build. The MVP is: it links the core via a `[lib]`+`bin` split, sends one message through `Agent::process`, renders the real reply, surfaces API errors instead of failing silently, gates on a missing API key, and locks tool execution behind explicit consent.
- **[P1 / Fast-follow]** — built after the MVP lands; behaviorally specified here so the contract is fixed.
- **[P2 / Later]** — gated on underlying core modules becoming real; specified at a high level only.

The following are **explicitly NOT in v1** and MUST NOT be advertised as working in the desktop UI: the animated/voiced mascot, real token-by-token LLM streaming, multi-agent / sub-agent (`task`) panels, goal deliberation (`goal_deliberate`), plan mode, the channels/gateway dashboard, voice, and provider profiles. Core handlers for `goal_deliberate`, `task`, `plan_mode`, and `perm` are placeholder text today and MUST NOT be exposed as functional features.

## ADDED Requirements

### Requirement: Library Target Prerequisite And CLI Preservation

The repository SHALL expose the agent core as a Rust library target (`[lib] name = "open_assistant"`) so that a separate `src-tauri/` crate can `use open_assistant::{core, config, tools, ...}` in-process, WITHOUT regressing the existing `clap`-based CLI binary. This is a hard, isolated prerequisite and SHALL land before any Tauri code. [P0 / MVP]

#### Scenario: Core is importable as a library

- **WHEN** the workspace is built with `cargo build`
- **THEN** a library target named `open_assistant` is produced that publicly re-exports at least `core`, `config`, `tools`, `memory`, and `skills`
- **AND** an external crate can reference `open_assistant::core::agent::Agent` and `open_assistant::config` without modification to `main.rs`'s public behavior

#### Scenario: Existing CLI still works after the refactor

- **WHEN** the lib/bin split is complete and `cargo run -- tui`, `cargo run -- status`, and `cargo run -- web` are each executed
- **THEN** each subcommand starts and behaves exactly as before the refactor
- **AND** `cargo build` and `cargo check` succeed with no new errors

#### Scenario: Refactor is an isolated, bisectable commit

- **WHEN** the lib/bin split is reviewed
- **THEN** it is a standalone commit (or branch) that introduces no Tauri dependency and no `src-tauri/` directory
- **AND** the commit can be verified green independently of any desktop work

### Requirement: Tauri Desktop Shell And Window

The desktop application SHALL be a Tauri 2.x crate under `src-tauri/` that depends on the `open_assistant` library, owns the application event loop, and presents a single primary window matching the OpenHumans layout: a left navigation rail (Chat, Memory, Integrations, Skills, Settings), a main content area, and — inside Chat — a 280px session/info sidebar. The Tauri-bundled OS WebView (WebView2 / WebKitGTK / WKWebView) is an explicit, documented exception to the project's single-binary constraint. [P0 / MVP for shell + Chat nav; other nav destinations land per their own requirements]

#### Scenario: App launches and shows the shell

- **WHEN** the packaged desktop app is launched on Windows, macOS, or Linux
- **THEN** a primary window opens with the left navigation rail and the Chat view selected by default
- **AND** the window renders the OpenHumans tokens (Inter font; primary `#2563eb`; amber accent `#f59e0b`; light background `#f8fafc`; border `#e2e8f0`)

#### Scenario: Navigation between top-level sections

- **WHEN** the user clicks a nav-rail item (e.g. `data-testid="nav-memory"`)
- **THEN** the main content area swaps to that section without reloading the window
- **AND** the active nav item is visually highlighted

#### Scenario: Single instance

- **WHEN** the user launches the app while an instance is already running
- **THEN** the existing window is focused and raised instead of a second process starting

#### Scenario: WebView exception is documented

- **WHEN** a reviewer checks the project docs against the single-binary constraint
- **THEN** there is a written note that the Tauri desktop shell intentionally bundles the OS WebView and is exempt from that constraint

### Requirement: Wired Agent Chat (Non-Streaming)

The Chat view SHALL send the user's message to the real agent loop by invoking `open_assistant::core::agent::Agent::process(message, ctx, session)` through an async `#[tauri::command]`, and render the returned assistant `Message`. The desktop app MUST NOT copy the existing `ui::web.rs` / `ui::tui.rs` chat handlers, which return hardcoded "simulated"/"In a full implementation..." placeholder strings and never call the agent. v1 is non-streaming: the command returns the final assistant `Message`; a typing/working indicator is shown while the request is in flight. [P0 / MVP]

#### Scenario: Sending a message returns the real assistant reply

- **WHEN** the user types text into the chat input (`data-testid="chat-input"`) and activates Send (`data-testid="chat-send"`)
- **THEN** the frontend invokes the `send_message` command with the text
- **AND** the command calls `Agent::process(...)` against the configured model/endpoint and returns the resulting assistant `Message` (id, role, content, timestamp)
- **AND** the message is appended to the message list (`data-testid="message-list"`) as an assistant bubble

#### Scenario: Working indicator during a turn

- **WHEN** a `send_message` invocation is in flight
- **THEN** a typing/working indicator is shown and the Send control is disabled
- **AND** the indicator is removed and the input re-enabled once the command resolves or errors

#### Scenario: No placeholder/simulated text path exists

- **WHEN** the desktop chat path is exercised end-to-end
- **THEN** no response contains the strings "simulated response" or "In a full implementation"
- **AND** the response content originates from the model returned by `Agent::process`

### Requirement: Single-Turn Concurrency Lock

The shared agent state (`FullContext` + `Session`, plus side-effecting per-turn behavior) SHALL be guarded by a **single async mutex** so that overlapping `send_message` invocations cannot interleave session writes or duplicate per-turn side effects (daily-note append, `ctx.observe`). The mutex MUST be `tokio::sync::Mutex` (or `tauri::async_runtime::Mutex`), NOT `std::sync::Mutex`, because its guard is held across the `Agent::process(...).await`. [P0 / MVP]

#### Scenario: Combined state behind one async mutex

- **WHEN** the Tauri managed state is constructed
- **THEN** `FullContext` and `Session` live inside one combined struct guarded by a single `tokio::sync::Mutex` (not two separate mutexes)
- **AND** the command holds that lock for the full duration of an `Agent::process` turn

#### Scenario: Rapid double-send does not interleave

- **WHEN** the user activates Send twice in quick succession (e.g. double Enter)
- **THEN** the second turn does not begin until the first turn's lock is released
- **AND** the session message order remains `user, assistant, user, assistant` with no duplicated or interleaved entries

#### Scenario: Async guard compiles in the command

- **WHEN** the project is built
- **THEN** the command holding the lock across `.await` compiles (no `!Send` guard error), confirming an async-aware mutex is used

### Requirement: API Error Surfacing And Empty-Key Gate

The desktop app SHALL surface real LLM transport/HTTP errors as visible UI errors instead of rendering a blank assistant bubble, and SHALL detect a missing API key on launch and route the user to Settings/onboarding before chat is attempted. This requires `call_llm` to check the HTTP response status before parsing, because the default config ships with an empty `model.api_key` and the current code parses `resp.json()` unconditionally, turning a 401 into a silent empty reply. [P0 / MVP]

#### Scenario: HTTP error is shown, not swallowed

- **WHEN** the LLM endpoint returns a non-success status (e.g. 401, 429, 5xx)
- **THEN** `call_llm` returns a typed error including the status code and response body excerpt rather than an empty string
- **AND** the Chat view renders a visible, dismissible error notice (`data-testid="chat-error"`) describing the failure
- **AND** no empty/blank assistant bubble is appended

#### Scenario: Missing API key routes to Settings on launch

- **WHEN** the app launches and `config.model.api_key` is empty
- **THEN** the user is shown the Settings/onboarding view (or a blocking banner) prompting them to enter an API key
- **AND** the Send control communicates that chat is unavailable until a key is provided

#### Scenario: Chat becomes available after a key is saved

- **WHEN** the user saves a non-empty API key from Settings
- **THEN** the app exits the onboarding gate and the Chat view becomes usable without restart

### Requirement: Tool Execution Consent And Safety Gate

Because `Agent::process` can dispatch real `bash`/file `read`/`write`/`edit`/`grep`/`glob` tools that run on the user's machine, and the existing `perm`/`plan_mode` handlers are non-enforcing placeholder text, the desktop app SHALL NOT expose a default-on, ungated shell/file-write surface. Tool execution SHALL be gated by an explicit, enforced consent boundary before the first side-effecting tool runs. The cosmetic `permission_mode` string is not sufficient. [P0 / MVP]

#### Scenario: Tools disabled or consented by default

- **WHEN** the app is first installed and run
- **THEN** side-effecting tools (`bash`, `write`, `edit`) are either disabled by default or require an explicit one-time consent before they may execute
- **AND** the consent state is enforced in the Rust core (not only displayed in the UI)

#### Scenario: Confirm before a side-effecting tool runs

- **WHEN** the agent attempts a side-effecting tool while consent has not been granted for that scope
- **THEN** the tool does not execute and the user is presented with a consent/confirmation affordance (`data-testid="tool-consent"`) describing the action
- **AND** the tool runs only after the user approves

#### Scenario: Windows shell availability is handled

- **WHEN** the agent invokes the `bash` tool on Windows where `bash` is not on `PATH`
- **THEN** the tool either routes to an available shell (PowerShell/cmd) or returns a clear, user-visible error
- **AND** the failure is not silently swallowed as an empty result

#### Scenario: Nested-JSON tool arguments are not silently truncated

- **WHEN** the model emits a tool call whose arguments contain nested braces/objects (e.g. a `write` whose `content` includes `{}`)
- **THEN** the desktop app does not silently degrade to empty arguments
- **AND** either the argument is parsed correctly or the user sees a visible parse error rather than a silent no-op

### Requirement: Settings And Model Configuration

The desktop app SHALL provide a Settings view that reads and writes the agent configuration (`~/.openassistant/config.yaml`) via Tauri commands, exposing at minimum `model.api_key` (masked), `model.model`, and `model.api_base`. Settings writes MUST mutate the loaded `Config` struct directly and call `config::save()`; they MUST NOT route through `config::set()`, which silently no-ops unknown keys such as `model.api_base`, `temperature`, and `max_tokens` via a warn-only fallback arm. Named provider profiles are explicitly deferred (NOT v1). [P0 / MVP for the three fields above; additional fields P1]

#### Scenario: Load current configuration into Settings

- **WHEN** the user opens the Settings view
- **THEN** the current `model.model`, `model.api_base`, and a masked `model.api_key` are displayed (`data-testid="settings-model"`, `settings-api-base`, `settings-api-key`)

#### Scenario: Save a field that config::set cannot handle

- **WHEN** the user edits `model.api_base` and saves
- **THEN** the change is persisted to `config.yaml` (verified by reloading)
- **AND** the persistence path mutates the `Config` struct and calls `config::save()` rather than calling `config::set("model.api_base", ...)`

#### Scenario: API key is never logged or shown in plaintext

- **WHEN** the API key field is rendered or its save is processed
- **THEN** the value is masked in the UI
- **AND** the key value does not appear in application logs

#### Scenario: Provider profiles are absent in v1

- **WHEN** the Settings view is inspected
- **THEN** there is no multi-provider-profile UI; provider switching in v1 is editing the three single fields above

### Requirement: Conversation Session Semantics

The desktop app SHALL define clear semantics for the in-memory conversation and for "clear / new conversation", and SHALL surface the truth that `Agent::process` writes daily-note markdown and mutates the `UserModel` on every message. v1 sessions are in-memory and lost on restart (no new SQLite history schema is built for the MVP, to avoid polluting the FTS memory store). [P0 / MVP for in-memory session + clear semantics; persistent multi-session history is P1]

#### Scenario: Clear conversation resets the visible session

- **WHEN** the user activates Clear (`data-testid="clear-conversation"`)
- **THEN** the visible message list is emptied and the managed `Session` is reset
- **AND** the UI indicates what "clear" does and does NOT affect (it does not erase already-persisted daily notes or the learned user model)

#### Scenario: In-memory session lost on restart (v1)

- **WHEN** the user restarts the app in v1
- **THEN** the prior conversation is not restored (in-memory only), which is the documented v1 behavior

#### Scenario: Persistent history (P1)

- **WHEN** session persistence ships (P1)
- **THEN** sessions are stored in a schema kept separate from `memory.db`'s FTS memory store
- **AND** a left-rail list shows past conversations with title, timestamp, and resume/delete actions

### Requirement: Status Telemetry Honesty

The desktop status surface SHALL display only telemetry the core actually produces. Token-in / token-out / cost fields MUST NOT be displayed as real values unless token accounting is implemented (capturing the API `usage` object); otherwise these fields SHALL be omitted rather than shown as permanent zeros presented as real data. [P0 / MVP]

#### Scenario: Status shows verifiable fields

- **WHEN** the user views the Chat session/info sidebar
- **THEN** it shows model name, permission/consent posture, workspace dir, and message count
- **AND** these values reflect the live managed state

#### Scenario: No fake token/cost display

- **WHEN** token accounting is not implemented
- **THEN** token-in, token-out, and cost are not displayed as real metrics (they are omitted or clearly labeled as unavailable)

### Requirement: Tauri Security Posture (CSP And Capabilities)

The desktop app SHALL ship a locked-down security posture using modern Tauri 2.x idioms: an explicitly set Content-Security-Policy (Tauri 2 ships CSP disabled by default) and a default-deny capabilities configuration granting only the specific permissions used. Because the Rust backend (`call_llm`) makes the LLM network request, the WebView itself needs no outbound network access and the CSP MUST NOT add the LLM origin to `connect-src`. [P0 / MVP]

#### Scenario: CSP is explicitly set and restrictive

- **WHEN** `tauri.conf.json` is inspected
- **THEN** `app.security.csp` is set (not null) to a restrictive policy such as `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'`
- **AND** the policy contains no `connect-src` entry for the LLM/provider origin (the WebView never calls it directly)

#### Scenario: Default-deny capabilities

- **WHEN** `src-tauri/capabilities/*.json` is inspected
- **THEN** it grants `core:default` plus only the per-plugin permissions actually used (e.g. global-shortcut, tray, window-state, single-instance), scoped to the relevant window label
- **AND** no blanket/wildcard permission grant is present

#### Scenario: Tool gating is enforced in the core, not via capabilities

- **WHEN** reviewing where shell/file tool access is restricted
- **THEN** the gate lives in the agent core (per the Tool Execution Consent requirement), because tools run inside the Rust core and Tauri capabilities cannot constrain them

### Requirement: Light/Dark Theming Matching OpenHumans

The desktop app SHALL implement theming via CSS variables seeded from the OpenHumans tokens, providing a light theme (default) and a dark theme, with the choice persisted and respecting the OS `prefers-color-scheme` on first run. The light theme is the canonical visual source of truth (ported from `ui/web.rs` `INDEX_HTML`); OpenHumans parity is "spirit, not pixel-match" given the reference palette is inferred. [P1 / Fast-follow — MVP ships light theme only]

#### Scenario: Tokens drive the theme

- **WHEN** the app renders in light mode
- **THEN** colors derive from CSS variables: primary `#2563eb`, accent `#f59e0b`, background `#f8fafc`, border `#e2e8f0`, with Inter as the font

#### Scenario: First run respects OS preference

- **WHEN** the app is launched for the first time with no saved theme
- **THEN** it selects light or dark based on the OS `prefers-color-scheme`

#### Scenario: Toggle persists across restart

- **WHEN** the user toggles the theme (`data-testid="theme-toggle"`) and restarts the app
- **THEN** the previously chosen theme is restored

### Requirement: Memory Browser

The desktop app SHALL provide a read-first Memory view backed by the existing dual memory systems: the file memory (`MEMORY.md`, `memory/YYYY-MM-DD.md` daily notes, `DREAMS.md`) and the SQLite+FTS5 `MemoryStore` (full-text search, list-by-category). Editing memory is deferred. [P1 / Fast-follow]

#### Scenario: Browse file memory

- **WHEN** the user opens the Memory view
- **THEN** a two-pane browser lists memory files/entries and renders the selected entry's content read-only (`data-testid="memory-list"`, `memory-content`)

#### Scenario: Full-text search the SQLite store

- **WHEN** the user enters a query in the memory search box (`data-testid="memory-search"`)
- **THEN** results come from `MemoryStore` FTS search and are listed with their category
- **AND** the conversation/session data and the FTS memory store remain in separate schemas (search does not return raw chat logs unless intentionally stored)

### Requirement: System Tray

The desktop app SHALL provide a system tray icon with a menu (Open, Quick Ask, Quit) using Tauri 2.x's tray API, with the corresponding capability permission entry. Tray behavior is verified on Windows and Linux; macOS is best-effort. [P1 / Fast-follow]

#### Scenario: Tray menu actions

- **WHEN** the user activates the tray "Open" item
- **THEN** the primary window is shown and focused
- **WHEN** the user activates the tray "Quit" item
- **THEN** the application exits cleanly

#### Scenario: Tray capability is declared

- **WHEN** capabilities are inspected
- **THEN** only the tray permission needed is granted, consistent with the default-deny posture

### Requirement: Global Hotkey Quick-Ask

The desktop app SHALL register a configurable global shortcut (default e.g. `Ctrl/Cmd+Shift+Space`) via `tauri-plugin-global-shortcut` that toggles a small Raycast-style quick-ask window; submitting a prompt there runs one `Agent::process` turn (subject to the same concurrency lock and tool-consent gate) and shows the answer. The shortcut and quick-ask window each require their capability entries. [P1 / Fast-follow]

#### Scenario: Hotkey toggles quick-ask

- **WHEN** the user presses the configured global shortcut
- **THEN** a compact quick-ask window appears focused with an input (`data-testid="quickask-input"`)
- **AND** pressing it again (or Escape) hides the window

#### Scenario: Quick-ask runs a real turn

- **WHEN** the user submits a prompt in quick-ask
- **THEN** the prompt is processed via the same `send_message` path and the answer is rendered in the quick-ask window
- **AND** the same turn lock prevents overlap with a main-window turn

#### Scenario: Shortcut conflict handling

- **WHEN** the configured shortcut cannot be registered (already taken by the OS/another app)
- **THEN** the app surfaces a non-fatal notice rather than crashing, and the rest of the app remains usable

### Requirement: Streaming Chat (Deferred Contract)

When real token-by-token streaming is implemented, the chat command SHALL accept a `tauri::ipc::Channel<StreamEvent>` argument and `call_llm` SHALL be refactored to consume the provider's SSE byte stream (`stream: true`) and send each delta over the channel, reusing the existing `src/core/streaming.rs` `StreamEvent` enum (annotated `#[serde(rename_all = "camelCase")]`) as the shared wire schema for CLI NDJSON and desktop. Per-token `Window::emit` is NOT the chosen mechanism. Streaming is explicitly NOT in v1. [P1 / Fast-follow]

#### Scenario: Streamed deltas render incrementally

- **WHEN** streaming is enabled and a message is sent
- **THEN** assistant text renders incrementally as `StreamEvent` deltas arrive over the channel
- **AND** a Stop control can cancel the in-flight stream

#### Scenario: Shared wire schema

- **WHEN** the streaming payload type is inspected
- **THEN** it is the same `StreamEvent` enum used by the CLI NDJSON path, with camelCase serialization

### Requirement: E2E And Test Strategy

The desktop app SHALL be testable via a defined, cross-platform-realistic strategy and SHALL include stable `data-testid` attributes on key interactive elements from day one. The strategy explicitly accounts for the fact that official Tauri E2E is WebDriver via `tauri-driver` on Windows + Linux only (macOS has no WKWebView driver), so native cross-platform E2E parity is not promised. [P0 / MVP for Rust tests + data-testids; native E2E harness P1]

#### Scenario: Stable test hooks exist

- **WHEN** the chat input, send button, message list, settings fields, error notice, and tool-consent affordance are inspected
- **THEN** each carries a stable `data-testid` attribute

#### Scenario: Rust unit/integration tests for the command layer

- **WHEN** `cargo test` is run
- **THEN** tests cover at least a config round-trip (load → mutate field → save → reload) and that `send_message` returns a `Message` (mockable transport), and they run on all platforms

#### Scenario: Native smoke tests gated to Windows and Linux

- **WHEN** native E2E runs in CI
- **THEN** `tauri-driver` + WebdriverIO/Selenium smoke tests (app loads, send message renders a reply, open Settings) run on Windows and Linux only
- **AND** macOS native E2E is not claimed; macOS coverage relies on Rust tests and optional browser-based UI tests

#### Scenario: Browser-based UI logic tests (optional)

- **WHEN** UI flows are tested without the native shell
- **THEN** Playwright (or equivalent) may exercise the dev web frontend in Chromium for UI logic, distinct from the native `tauri-driver` smoke tests
