## ADDED Requirements

### Requirement: First-Run Onboarding Wizard

On first launch — defined as `config.model.api_key` being empty **or** the full `Config` struct being entirely default-valued — the desktop app SHALL present a multi-step onboarding wizard as a full-screen, non-dismissible overlay over the app shell rather than routing to Settings. The wizard collects the minimum information needed to start chatting, writes all fields through one `save_onboarding_config` Tauri command that loads the full `Config`, mutates struct fields directly, and calls `config::save()` — never routing through `config::set()`. The wizard MUST NOT call `src/onboarding/wizard.rs` `run_wizard()`, which uses blocking `stdin` and cannot be invoked from the Tauri webview. A `get_app_state` command (or equivalent extension to `get_config`) SHALL return an `initial_view` field (`"onboarding"` | `"chat"`) before first paint so the frontend routes without a flash of the wrong view. [P1 / Fast-follow; the existing API-key gate from task 4.2 is P0 / MVP and remains the launch-blocking gate]

#### Scenario: Wizard is shown on a fresh install

- **WHEN** the app launches and `config.model.api_key` is empty
- **THEN** the frontend displays the onboarding wizard overlay (`data-testid="onboarding-wizard"`) rather than the Settings view
- **AND** the nav rail is visible behind the overlay but all items are `aria-disabled="true"` and non-interactive
- **AND** clicking outside the wizard card does NOT dismiss it

#### Scenario: Wizard overlay is not dismissible without completing or skipping through

- **WHEN** the onboarding wizard is open
- **THEN** there is no close button, backdrop-click dismissal, or Escape-key handler that closes the wizard
- **AND** the user can only leave the wizard by completing the final screen's CTA or, on optional screens, using an explicit Skip affordance

#### Scenario: Wizard pre-fills existing config on re-run

- **WHEN** the wizard is opened from the Settings "Re-run Setup Wizard" entry point while a config already exists
- **THEN** every wizard screen pre-fills its fields from the current `Config` values
- **AND** the API key field is rendered as a masked placeholder (`••••...••••`) with a "Change" button that, when clicked, clears the field and re-locks the Continue button until the connection test passes again
- **AND** navigating back preserves all field values entered so far

#### Scenario: get_app_state routes the frontend before first paint

- **WHEN** the app JavaScript initializes on load
- **THEN** it invokes `get_app_state` (or the `initial_view` field of `get_config`) before rendering any view
- **AND** when `initial_view` is `"onboarding"`, the wizard overlay is mounted immediately
- **AND** when `initial_view` is `"chat"`, the Chat view is shown directly, identical to the existing post-setup behavior

#### Scenario: Wizard routes to Chat on completion

- **WHEN** the user activates the final "Start chatting" CTA on the wizard finish screen
- **THEN** `save_onboarding_config` is invoked with all collected fields, writing them to `~/.openassistant/config.yaml`
- **AND** the wizard overlay is dismissed
- **AND** the app navigates to the Chat view with the Send control enabled

#### Scenario: Wizard is re-enterable from three points

- **WHEN** the user opens Settings
- **THEN** a "Re-run Setup Wizard" secondary button is visible below the Save button (`data-testid="settings-rerun-wizard"`)
- **WHEN** the user views the Status / Doctor panel and the API key is empty
- **THEN** a "Run Setup Wizard" action link is visible (`data-testid="doctor-run-wizard"`)
- **WHEN** the app launches with an empty API key and the existing API-key-gate banner is shown
- **THEN** a "Run Setup Wizard" link appears alongside the "Go to Settings" button (`data-testid="gate-run-wizard"`)

---

### Requirement: Onboarding Wizard — Screen 1: Workspace

The first wizard screen SHALL collect `general.data_dir` and verify that the chosen path is writable. [P1]

#### Scenario: Workspace screen displays a pre-filled default path

- **WHEN** Screen 1 is shown
- **THEN** it displays the OS-appropriate default data directory (`%USERPROFILE%\.openassistant` on Windows; `~/.openassistant` on macOS/Linux) in a styled path chip (`data-testid="onboard-workspace-path"`)
- **AND** the path is shown with OS-native path separators (backslash on Windows, forward slash elsewhere)
- **AND** a "Change folder" button (`data-testid="onboard-workspace-change"`) is present

#### Scenario: Native folder picker opens on "Change folder"

- **WHEN** the user clicks "Change folder"
- **THEN** the OS native directory picker opens via `pick_data_dir` (backed by `tauri-plugin-dialog` `blocking_pick_folder`, wrapped in `spawn_blocking`)
- **AND** the selected path replaces the chip's displayed path

#### Scenario: Writability check gates the Continue button

- **WHEN** Screen 1 is displayed or after the user selects a new path
- **THEN** `check_path_writable` is invoked, which creates the directory if absent and tests write access
- **AND** while the check is pending, Continue is disabled and a spinner is shown in the writability badge
- **AND** if the path is writable, a green "Writable" badge appears and Continue is enabled
- **AND** if the path is not writable, a red inline error appears (`data-testid="onboard-workspace-error"`) and Continue remains disabled

#### Scenario: Workspace subdirectories are created on advance

- **WHEN** the user advances from Screen 1 to Screen 2
- **THEN** `check_path_writable` has confirmed the path is writable
- **AND** the `skills/` and `memory/` subdirectories within `data_dir` are created if they do not exist, so that later wizard steps can reliably write to them

---

### Requirement: Onboarding Wizard — Screen 2: AI Provider and API Key

The second wizard screen SHALL collect `model.provider`, `model.model`, `model.api_base`, and `model.api_key`, and SHALL gate the Continue button on a passing connection test. [P1]

#### Scenario: Provider selector auto-fills api_base and model

- **WHEN** Screen 2 is shown
- **THEN** three provider radio cards are displayed (`data-testid="onboard-provider-radio"`) for OpenRouter (recommended), OpenAI, and Custom
- **AND** selecting OpenRouter pre-fills `api_base` with the OpenRouter endpoint and `model` with the recommended default
- **AND** selecting OpenAI pre-fills `api_base` with the OpenAI endpoint and `model` with a sensible OpenAI default
- **AND** selecting Custom shows an editable `api_base` text field (`data-testid="onboard-api-base"`) that the user must fill

#### Scenario: API key field is masked with show/hide

- **WHEN** the user types into the API key field (`data-testid="onboard-api-key"`)
- **THEN** the value is masked by default (type="password")
- **AND** a show/hide eye-icon toggle is present to reveal the value
- **AND** editing the API key field clears any cached connection-test result and re-locks the Continue button

#### Scenario: External provider links open in the OS browser

- **WHEN** the user clicks a "Get an OpenRouter key →" helper link
- **THEN** the link is opened in the OS default browser via `tauri-plugin-opener` `open_url`, scoped to specific provider domains in `capabilities/default.json`
- **AND** the link does NOT navigate within the WebView, preserving the locked CSP

#### Scenario: Continue is locked until the connection test passes

- **WHEN** Screen 2 is shown with a non-empty API key
- **THEN** the Continue button is disabled until the connection test result is `success`
- **AND** editing the `api_key`, `api_base`, or `model` field invalidates any prior passing test result and re-disables Continue

---

### Requirement: Onboarding Wizard — Connection Test (Inline in Screen 2)

The wizard SHALL provide a "Test connection" button inline on Screen 2 that fires a real API probe via a dedicated `probe_connection` Tauri command before allowing the user to advance. [P1]

#### Scenario: probe_connection is a raw reqwest call, not Agent::process

- **WHEN** `probe_connection(api_key, api_base, model)` is invoked
- **THEN** it sends a POST to `{api_base}/chat/completions` using a direct `reqwest::Client` call with `Authorization: Bearer {api_key}`, `model`, `messages: [{role: "user", content: "hi"}]`, and `max_tokens: 1`
- **AND** it does NOT call `Agent::process` and therefore does NOT trigger a daily-note write, `UserModel` mutation, or any other per-turn side effect
- **AND** it returns a `ProbeResult { ok: bool, latency_ms: u32, error_type: Option<String> }`

#### Scenario: Success state unlocks Continue

- **WHEN** `probe_connection` returns `ok: true`
- **THEN** a green success banner appears (`data-testid="onboard-probe-success"`) showing the latency in milliseconds
- **AND** the Continue button is enabled

#### Scenario: Auth failure shows a specific error

- **WHEN** `probe_connection` returns a 401 or 403 HTTP status
- **THEN** a red inline error appears (`data-testid="onboard-probe-error"`) with the message "Invalid API key. Check the value and try again."
- **AND** the Continue button remains disabled

#### Scenario: Network failure shows a specific warning

- **WHEN** `probe_connection` returns a connection error, DNS failure, timeout, or 5xx response
- **THEN** an amber inline warning appears (`data-testid="onboard-probe-warning"`) with the message "Could not reach the endpoint. Check the API base URL and your network."
- **AND** a "Try again" affordance is present
- **AND** the Continue button remains disabled

#### Scenario: Testing spinner during probe

- **WHEN** the user clicks "Test connection" (`data-testid="onboard-test-connection"`)
- **THEN** the button label changes to a loading state with a spinner
- **AND** the `api_key` and `api_base` fields are read-only while the probe is in flight

---

### Requirement: Onboarding Wizard — Screen 3: Tools and Permissions Consent

The third wizard screen SHALL obtain an explicit, non-skippable tool-consent decision from the user before the first chat session begins. This screen is a dedicated wizard step and is not hidden in Settings. [P1]

#### Scenario: Consent screen requires an explicit choice before Continue

- **WHEN** Screen 3 is displayed
- **THEN** the Continue button is disabled until the user has clicked one of the two option cards (`data-testid="onboard-tools-card-a"` for Chat Only; `data-testid="onboard-tools-card-b"` for Enable Tools)
- **AND** "Chat Only" (no file or shell access) is the pre-highlighted default

#### Scenario: Chat Only selection disables tools

- **WHEN** the user selects "Chat Only" and completes the wizard
- **THEN** `tools_enabled` is saved as `false` and the agent's `handle_tool_calls` dispatch remains bypassed for this installation
- **AND** no secondary warning is shown

#### Scenario: Enable Tools selection surfaces a warning and persists the choice

- **WHEN** the user selects "Enable Tools"
- **THEN** an amber warning banner appears below the cards informing the user that the assistant can run shell commands and read/write files, and that this can be changed in Settings
- **AND** when the wizard completes, `tools_enabled: true` is written to the persisted `Config` (in a `tools.enabled` field or an equivalent persisted mechanism) so the choice survives app restart
- **AND** the `tools_enabled` flag in managed `DesktopState` is also updated

#### Scenario: tools_enabled persists across app restart

- **WHEN** the user has chosen "Enable Tools" in the wizard (or toggled it in Settings) and restarts the app
- **THEN** the `tools_enabled` state is read from `Config` on startup rather than defaulting to `false` every launch
- **AND** the tools opt-in or opt-out choice is not lost

---

### Requirement: Onboarding Wizard — Screen 4: Identity and Finish

The fourth and final wizard screen SHALL collect an optional `general.user_name`, display live background-check status pills, and provide the "Start chatting" CTA. [P1]

#### Scenario: Name input is optional

- **WHEN** Screen 4 is shown
- **THEN** a text input for the user's preferred name is displayed (`data-testid="onboard-username"`) with placeholder "friend" and a maximum of 64 characters
- **AND** a "Skip" affordance is present that advances without writing a user name (the default "friend" is used)
- **AND** activating "Start chatting" with a blank or whitespace-only name behaves identically to Skip

#### Scenario: Background status pills resolve asynchronously

- **WHEN** Screen 4 mounts
- **THEN** three status pills are shown: "Config saved", "Data directory ready", and "Vision tools" (`data-testid="onboard-pill-config"`, `onboard-pill-datadir"`, `"onboard-pill-vision"`)
- **AND** each pill starts in a pending/spinner state and updates independently as its check completes
- **AND** the "Vision tools" pill calls `tools::vision::check()` with a 200 ms timeout; if `gemini` is absent or the check times out, the pill shows an amber "Not found — image analysis unavailable" state, which is presented as informational and does NOT block the CTA
- **AND** "Start chatting" is enabled immediately on mount and does not wait for the pills to resolve

#### Scenario: Start chatting saves user_name and closes the wizard

- **WHEN** the user activates "Start chatting" (`data-testid="onboard-finish-cta"`)
- **THEN** the trimmed `user_name` value (or `"friend"` if blank) is included in the `save_onboarding_config` call and written to `config.general.user_name`
- **AND** the wizard overlay is dismissed and the Chat view is shown

#### Scenario: Step progress bar is accurate

- **WHEN** the wizard is open on any screen
- **THEN** a horizontal step bar at the top of the wizard card (`data-testid="onboard-step-bar"`) shows exactly four numbered steps: "1 Workspace · 2 AI Provider · 3 Permissions · 4 Finish"
- **AND** completed steps are filled with the primary color (`#2563eb`), the current step has an animated ring, and future steps are grey

---

### Requirement: save_onboarding_config Command

A dedicated `save_onboarding_config` Tauri command SHALL atomically write all onboarding-collected fields to `config.yaml` via the full load/mutate/save pattern, and SHALL be the single authoritative write path for the wizard. [P1]

#### Scenario: Command writes all wizard fields in one call

- **WHEN** `save_onboarding_config` is invoked with `{ data_dir, user_name, provider, model, api_base, api_key, tools_enabled }`
- **THEN** it calls `config::load()`, mutates `general.data_dir`, `general.user_name`, `model.provider`, `model.model`, `model.api_base`, and `model.api_key` on the loaded struct, and calls `config::save(&cfg)`
- **AND** it does NOT call `config::set()` at any point
- **AND** after the call, a `config::load()` round-trip reflects every written field

#### Scenario: api_key is only overwritten when non-empty

- **WHEN** `save_onboarding_config` is invoked with an empty or absent `api_key`
- **THEN** the existing `model.api_key` in `config.yaml` is preserved unchanged

#### Scenario: skills.dirs uses the default derived from data_dir

- **WHEN** `save_onboarding_config` is invoked
- **THEN** `config.skills.dirs` is set to `["{data_dir}/skills"]` server-side (not from a comma-separated string passed from the frontend) so it is stored as a proper `Vec<String>` in the YAML

---

### Requirement: check_path_writable and pick_data_dir Commands

Two utility Tauri commands SHALL support the workspace screen: one to verify and prepare a data directory, and one to open the native folder picker. [P1]

#### Scenario: check_path_writable creates and verifies the directory

- **WHEN** `check_path_writable(path: String)` is invoked
- **THEN** it creates the directory (and all parents) if absent, attempts to write a temporary file, removes it, and returns `Ok(true)` on success or `Err(message)` if the path is not writable
- **AND** the operation is non-destructive: it does not modify any existing files in the directory

#### Scenario: pick_data_dir returns a native OS path string

- **WHEN** `pick_data_dir` is invoked
- **THEN** it opens the OS native directory picker using `tauri-plugin-dialog`'s `blocking_pick_folder`, wrapped in `tauri::async_runtime::spawn_blocking` to avoid blocking the Tokio executor
- **AND** the returned path is converted via `.as_path().map(|p| p.display().to_string())` on `FilePath` (NOT `.to_string()` on the enum variant) to yield a proper OS-native path string
- **AND** it returns `Ok(None)` if the user cancels the picker

---

### Requirement: Expanded Settings View

The Settings view SHALL be restructured from a single flat form into a sectioned two-column layout (category sidebar + content panel) exposing the full range of real, non-stub `Config` fields. [P1]

#### Scenario: Settings has a category sidebar

- **WHEN** the user opens the Settings view
- **THEN** a category sidebar (approximately 140 px wide) lists at minimum: Model, General, Tools, Memory, Skills, Channels, Advanced (`data-testid="settings-nav"`)
- **AND** clicking a category shows its fields in the right content panel without a full page reload

#### Scenario: General section exposes user_name and data_dir

- **WHEN** the user selects the General category
- **THEN** a `user_name` text field (`data-testid="settings-user-name"`) and a read-only `data_dir` display with an "Open folder" button are shown
- **AND** saving a changed `user_name` persists it via the full load/mutate/save path to `config.general.user_name`

#### Scenario: Channels section shows gateway token fields with an experimental callout

- **WHEN** the user selects the Channels category
- **THEN** masked text fields for `gateway.discord_token`, `gateway.telegram_token`, and `gateway.slack_token` are shown, each masked identically to `api_key` (`data-testid="settings-discord-token"`, `"settings-telegram-token"`, `"settings-slack-token"`)
- **AND** a visible callout reads "Gateway integration is experimental — the messaging server is not yet fully operational" so users are not misled into believing Discord/Telegram bots are ready to use
- **AND** saving any of these fields persists them via the full load/mutate/save path

#### Scenario: Advanced section is collapsed by default

- **WHEN** the user opens Settings without selecting Advanced
- **THEN** the Advanced category is collapsed behind a disclosure control and its fields are not visible
- **AND** when expanded, it exposes `vision.provider`, `vision.gemini_path`, and `general.log_level`

#### Scenario: Settings save always uses load/mutate/save, never config::set()

- **WHEN** any Settings field is saved
- **THEN** the backing Tauri command loads the full `Config`, mutates only the changed field(s) on the struct, and calls `config::save()`
- **AND** the command does not call `config::set()` at any point

---

### Requirement: Memory Browser

The Memory nav rail item SHALL be enabled and wired to a read-first browser backed by `MemoryWorkspace` (file memory) and `MemoryStore` (SQLite/FTS5). All operations are backed by real `src/core/memory.rs` I/O — there are no stubs in this path. [P1]

#### Scenario: File memory is listed and readable

- **WHEN** the user opens the Memory view
- **THEN** the left pane (`data-testid="memory-list"`) shows `MEMORY.md`, `DREAMS.md`, and any `memory/YYYY-MM-DD.md` daily notes found by `MemoryWorkspace` methods
- **AND** selecting an entry renders its markdown content read-only in the right pane (`data-testid="memory-content"`)

#### Scenario: MEMORY.md is editable

- **WHEN** the user views `MEMORY.md` in the Memory browser
- **THEN** an Edit button transitions the right pane to an editable textarea (`data-testid="memory-edit-area"`)
- **AND** saving calls `write_memory_md` which invokes `MemoryWorkspace::write_long_term()` from `src/core/memory.rs`
- **AND** daily note files (`memory/YYYY-MM-DD.md`) are displayed read-only and have no Edit button

#### Scenario: Today's daily note is shown read-only

- **WHEN** the user selects today's daily note entry
- **THEN** the content is loaded via `get_today_note` which calls `MemoryWorkspace::read_today()`
- **AND** no edit or append UI is shown for daily notes in the browser (daily notes are written by the agent, not manually)

#### Scenario: FTS5 search returns structured results

- **WHEN** the user types a query into the memory search box (`data-testid="memory-search"`)
- **THEN** results are returned by `search_memory_db` which calls `MemoryStore::open_default().await` and `store.search_fts(query, limit)`
- **AND** each result shows its key, category, and a content excerpt
- **AND** search results do not include raw chat session logs unless they were explicitly stored in `MemoryStore`

#### Scenario: Tauri commands for memory use real MemoryWorkspace and MemoryStore APIs

- **WHEN** any memory Tauri command is invoked (`get_memory_md`, `write_memory_md`, `get_today_note`, `search_memory_db`, `list_memory_files`)
- **THEN** it calls the corresponding method on `MemoryWorkspace` or `MemoryStore` from `src/core/memory.rs` or `src/memory/store.rs`
- **AND** no placeholder or hardcoded strings are returned

---

### Requirement: Skills Manager

The Skills nav rail item SHALL be enabled and wired to a skills browser that lists and reads real skill definitions from `SkillEngine`, and allows creating new skill files. [P1]

#### Scenario: Built-in and user skills are listed

- **WHEN** the user opens the Skills view
- **THEN** the left pane (`data-testid="skills-list"`) shows skills loaded by `SkillEngine::load_builtin()` (which loads the 3 built-in skills from embedded Markdown) and any user skills from `config.skills.dirs[0]` via `SkillEngine::load_from_dir()`
- **AND** each entry shows the skill name, description, and category

#### Scenario: Selecting a skill shows its content

- **WHEN** the user selects a skill entry
- **THEN** the right pane (`data-testid="skill-content"`) renders the skill's Markdown content
- **AND** the content is read from `read_skill` which calls `SkillEngine::get(name)` or reads the file directly for user-created skills

#### Scenario: New skill creation writes a real file

- **WHEN** the user clicks "New Skill" and submits a name and Markdown content
- **THEN** `create_skill(name, content)` is invoked, which calls `tokio::fs::write` to persist a `.md` file under `config.skills.dirs[0]`
- **AND** the new skill appears in the list immediately after creation

#### Scenario: Skill activation is not exposed

- **WHEN** the Skills view is inspected
- **THEN** there is no "Activate" toggle or button for any skill, because `SkillEngine::activate_skill()` is not wired into `Agent::process` and activating a skill in the UI would have no effect on agent behavior
- **AND** a note reads "Skill activation in chat is coming soon" to communicate the limitation without hiding the feature

---

### Requirement: Status and Doctor Diagnostics Panel

A Status panel (replacing or augmenting the existing Chat sidebar status) SHALL expose live system metrics and a run-on-demand Doctor diagnostics view. All checks call real code — there are no stubs in this path. [P1]

#### Scenario: Status panel shows extended real metrics

- **WHEN** the user opens the Status panel or nav item
- **THEN** `get_status` returns and the UI displays: model name, `api_base`, workspace directory, message count, `tools_enabled` state, `api_key_set` boolean, `memory_db_entries` (from `MemoryStore::open_default().await` + `store.count()`), `memory_md_chars` (from `MemoryWorkspace::from_data_dir().read_long_term().len()`), and `data_dir` path
- **AND** token counts and cost are NOT displayed because `call_llm` does not capture the provider `usage` block in the current implementation

#### Scenario: Doctor runs six real diagnostic checks

- **WHEN** the user clicks "Run diagnostics" (`data-testid="doctor-run"`)
- **THEN** `run_doctor` is invoked, which runs the following six checks in sequence and returns `Vec<DiagnosticResult { name: String, ok: bool, message: String }>`:
  1. Config load: calls `config::load()` and reports the config file path
  2. Memory DB: calls `MemoryStore::open_default().await` and verifies the schema initializes
  3. Memory workspace: calls `MemoryWorkspace::from_data_dir(&config.general.data_dir).init()` and verifies dirs and `MEMORY.md` exist
  4. Skills: calls `SkillEngine::load_builtin()?.count()` (NOT `SkillEngine::default().count()`, which returns 0) and reports the skill count
  5. Gateway: calls `gateway::check()` and reports whether tokens are configured
  6. Vision: calls `tools::vision::check()` with a 200 ms timeout; a missing `gemini` binary returns `ok: false` with message "Gemini CLI not found — image analysis unavailable", NOT a panic or unhandled error

#### Scenario: Doctor results are rendered as colored pass/fail rows

- **WHEN** `run_doctor` returns results
- **THEN** each `DiagnosticResult` is rendered as a row (`data-testid="doctor-row"`) with a green checkmark if `ok: true` or a red/amber indicator if `ok: false`
- **AND** the `message` string is shown alongside the indicator

#### Scenario: Missing gemini binary is treated as neutral, not a critical failure

- **WHEN** `tools::vision::check()` fails because `gemini` is not on `PATH` (common on Windows)
- **THEN** the Vision row is rendered with an amber "optional" indicator rather than a red "failure" indicator
- **AND** the overall diagnostics summary does not count a missing `gemini` binary as a blocking error

---

### Requirement: Tauri Plugin and Capability Additions for New Features

The new onboarding and surface features require additional Tauri 2.x plugins and capability entries. These SHALL be added to `src-tauri/Cargo.toml`, registered in `lib.rs` before `.setup()`, and declared in `src-tauri/capabilities/default.json`. [P1]

#### Scenario: tauri-plugin-dialog is added for folder picking

- **WHEN** the desktop app is built
- **THEN** `src-tauri/Cargo.toml` includes `tauri-plugin-dialog = "2"` as a direct dependency
- **AND** `lib.rs` registers `.plugin(tauri_plugin_dialog::init())` before `.setup()`
- **AND** `capabilities/default.json` includes `"dialog:default"` in the permissions array

#### Scenario: tauri-plugin-opener is added for external URLs

- **WHEN** the desktop app is built
- **THEN** `src-tauri/Cargo.toml` includes `tauri-plugin-opener = "2"` as a direct dependency
- **AND** `lib.rs` registers `.plugin(tauri_plugin_opener::init())` before `.setup()`
- **AND** `capabilities/default.json` includes a scoped `opener:allow-open-url` entry allowing specific provider domains (e.g., `https://openrouter.ai/*`, `https://openai.com/*`, `https://platform.openai.com/*`) rather than a blanket `https://*` wildcard

#### Scenario: reqwest is a direct dependency of src-tauri for probe_connection

- **WHEN** `src-tauri/Cargo.toml` is inspected
- **THEN** `reqwest = { version = "0.12", features = ["json"] }` is listed as a direct dependency (not relied upon as a transitive dependency of `open-assistant`, which is not directly usable in `src-tauri`'s own `use` statements)

#### Scenario: The single invoke_handler invariant is preserved

- **WHEN** new commands are added for onboarding, memory, skills, status, and doctor
- **THEN** all commands are registered in a SINGLE `tauri::generate_handler![]` call in `lib.rs`
- **AND** a code comment reads "SINGLE invoke_handler — Tauri discards all but the last registration; add new commands here"
- **AND** the commands directory is structured as `src-tauri/src/commands/` with submodules (e.g., `chat.rs`, `settings.rs`, `onboarding.rs`, `system.rs`, `memory.rs`, `skills.rs`) once the surface exceeds approximately 10 commands

#### Scenario: CSP remains locked despite new external-URL capability

- **WHEN** `tauri-plugin-opener` is added
- **THEN** `tauri.conf.json` `app.security.csp` is NOT updated to add any external origin to `connect-src`, `script-src`, or `default-src`
- **AND** the opener plugin opens URLs via the OS shell, not via WebView navigation, so no CSP relaxation is needed

---

### Requirement: Stubbed Core Features Are Not Presented as Functional

The desktop app SHALL NOT expose sub-agent execution, workflow execution, checkpoint restore, marketplace plugin install, or the Update self-updater as working features in any panel, button, or interactive element. Each of these has a verified stub implementation that returns fabricated success or placeholder text. [P0 / MVP — this is an explicit honesty constraint that applies to all phases]

#### Scenario: Sub-agent definitions are shown read-only

- **WHEN** an Agents view is present in the desktop app
- **THEN** it displays only the list of agent definitions loaded from `.claude/agents/` via `SubAgentOrchestrator::load_definitions()` and `list_definitions()` (which is real file I/O)
- **AND** there is no "Run", "Spawn", or "Execute" button on any agent definition
- **AND** a persistent notice reads "Agent execution is not yet available in the desktop app" (`data-testid="agents-stub-notice"`)
- **AND** `spawn_agent()` is not implemented as a Tauri command, because `execute_subagent()` returns a hardcoded string regardless of input

#### Scenario: Workflows are not presented as executable

- **WHEN** workflows are surfaced anywhere in the desktop app
- **THEN** `list_workflows()` may be exposed read-only to show available workflow definitions
- **AND** there is no "Run Workflow" button or command exposed in the UI, because `WorkflowEngine` step closures return only `"Step X completed: description"` strings without calling the LLM or doing real work
- **AND** any workflow display is labeled "Preview / coming soon"

#### Scenario: Checkpoints are session-only and labeled accordingly

- **WHEN** a checkpoint feature is present in the desktop app
- **THEN** a `CheckpointStore` in `AppCore` state (if added) is clearly labeled "Session snapshots — cleared when the app closes"
- **AND** the UI does not imply that checkpoints persist to disk or survive app restart, because `CheckpointStore` is in-memory only with no SQLite persistence despite the file comment claiming otherwise
- **AND** no "Restore" button is present until SQLite persistence is implemented

#### Scenario: Marketplace plugin install is not exposed

- **WHEN** a Plugins view is present
- **THEN** there is no "Install from marketplace" button or input, because `PluginSource::Marketplace` always returns `Err("Marketplace not yet configured")`
- **AND** only local-path plugin install (if implemented) is exposed
- **AND** a label "Marketplace coming soon" is present

#### Scenario: Plugin enable/disable state is labeled as session-only

- **WHEN** a plugin enable/disable toggle is present
- **THEN** the toggle is accompanied by a note "Enabled state resets on app restart" because `PluginMarketplace::set_enabled()` mutates in-memory state only and does not write back to `plugin.json`

#### Scenario: No Update button exists in the desktop app

- **WHEN** the Settings view is inspected
- **THEN** there is no "Check for updates" or "Update" button, because the CLI `Update` command only prints cargo instructions and performs no real self-update
- **AND** the Settings footer shows only the application version string

#### Scenario: A capability-honesty note is present in code

- **WHEN** the source code of the desktop app is reviewed
- **THEN** a clearly-labeled comment block (e.g., in `src-tauri/src/commands/mod.rs` or an equivalent location) lists each terminal feature as REAL, STUB, or NOT SURFACED, with a source-file line reference, so future contributors do not accidentally add a "Run" button to a stub feature

---

### Requirement: E2E Test Coverage for Onboarding and New Panels

The existing Playwright + `window.__MOCK_BACKEND__` + `data-testid` E2E harness SHALL be extended to cover all new wizard screens and panel commands. [P1]

#### Scenario: Mock backend covers all new commands

- **WHEN** the E2E test harness runs in mock mode (Chromium, no native Tauri shell)
- **THEN** `window.__MOCK_BACKEND__` (and the corresponding `defaultMock` in `frontend/app.js`) handles: `probe_connection` (returning success, auth-failure, and network-failure states), `check_path_writable`, `pick_data_dir`, `save_onboarding_config`, `get_memory_md`, `write_memory_md`, `get_today_note`, `search_memory_db`, `list_skills`, `read_skill`, `create_skill`, `get_status` (extended), and `run_doctor`
- **AND** each mock returns a plausible stub value that exercises the corresponding UI state

#### Scenario: Wizard flow is testable end-to-end in Playwright

- **WHEN** the Playwright suite runs the onboarding wizard flow
- **THEN** the test navigates through all four wizard screens using `data-testid` selectors: `onboard-step-bar`, `onboard-workspace-path`, `onboard-workspace-change`, `onboard-provider-radio`, `onboard-api-key`, `onboard-test-connection`, `onboard-probe-success`, `onboard-tools-card-a`, `onboard-tools-card-b`, `onboard-username`, `onboard-finish-cta`
- **AND** the test asserts that the wizard overlay is dismissed and the Chat view is shown after the CTA is activated

#### Scenario: Rust round-trip test for save_onboarding_config

- **WHEN** `cargo test` is run in `src-tauri/`
- **THEN** a `#[cfg(test)]` test invokes `save_onboarding_config` with all wizard fields, then calls `config::load()`, and asserts that `data_dir`, `user_name`, `provider`, `model`, `api_base`, and `skills.dirs` are persisted correctly
- **AND** the test asserts that an empty `api_key` in the call does not overwrite an existing key
- **AND** the test runs on Windows, Linux, and macOS in CI
