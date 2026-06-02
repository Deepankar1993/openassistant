# Implementation Tasks: add-desktop-onboarding-options

Cross-phase checklist for adding a proper onboarding wizard and surfacing the real (non-stub) terminal features in the openAssistant desktop app. This change builds on top of the already-committed `add-desktop-app` work (phases 1-6 of that change are checked). Items marked **[MVP]** are required for the first shippable build of this change. Everything else is P1 fast-follow within this change. Do not start P1 work until all **[MVP]** items are checked and `cargo build` (root + `src-tauri/`) still pass.

> **Scope guardrail (reconciled from expert panel):** This change ships (1) a guided 4-screen onboarding wizard, (2) expanded two-column Settings, (3) a Memory browser, (4) a Skills manager, and (5) a Status/Doctor panel. The following are **explicitly NOT surfaced in this change** — their core implementations are verified stubs or have critical gaps and MUST NOT be presented as working features:
>
> | Feature | Why not surfaced | Source reference |
> |---|---|---|
> | Sub-agent execution (`spawn_agent`) | `execute_subagent()` returns a hardcoded placeholder string | `src/core/subagent.rs:267-283` |
> | Workflow execution (`run_workflow`) | Each step outputs `"Step X completed: description"` with no real work | `src/core/workflows.rs:141-161` |
> | Checkpoint restore | `CheckpointStore::new()` is in-memory only; lost on restart | `src/core/checkpoint.rs:31` |
> | Plugin marketplace install | `PluginSource::Marketplace` always returns `Err("Marketplace not yet configured")` | `src/core/plugins.rs:216` |
> | Self-update button | `Update` command just prints `cargo update && cargo build` instructions | `src/main.rs:119` |
> | Skill activation toggle | `activate_skill()` is not checked anywhere in `Agent::process` | `src/skills/engine.rs` |
> | Live gateway/channel dashboard | Discord/Telegram/Slack gateways are placeholder implementations | `src/gateway/` |
> | Goal deliberation, plan mode, `perm` | All return placeholder text | `src/core/agent.rs:323,399,409` |
>
> See task 1.1 for the capability honesty comment block that enforces this in code.

---

## Phase 1 — Foundation: config persistence, plugin registration, command module structure

> These are mechanical prerequisites. Do them in one isolated branch/commit before any wizard UI work. Get `cargo build` (both crates) green before proceeding.

- [ ] 1.1 **[MVP]** Add a `/// CAPABILITY HONESTY TABLE` comment block to `src-tauri/src/lib.rs` (or a new `src-tauri/src/STUBS.md` linked from it) listing every stub feature from the scope guardrail above with its source-line reference and policy `NOT_SURFACED`. This prevents a future contributor from re-adding a "Run" button to agents/workflows/checkpoints. The comment is the authoritative gate, checked in code review.

- [ ] 1.2 **[MVP]** Add a `[tools]` section to `src/config/mod.rs` `Config` struct:
  ```rust
  #[derive(Debug, Serialize, Deserialize, Clone)]
  pub struct ToolsConfig {
      pub enabled: bool,
  }
  impl Default for ToolsConfig {
      fn default() -> Self { Self { enabled: false } }
  }
  ```
  Add `pub tools: ToolsConfig` to `Config` and add `tools: ToolsConfig::default()` to `Config::default()`. This persists the onboarding consent choice across restarts. The existing in-memory `tools_enabled` field on `DesktopState` reads its initial value from `config.tools.enabled` at startup (task 1.3).

- [ ] 1.3 **[MVP]** Update `build_core()` in `src-tauri/src/lib.rs` to read `cfg.tools.enabled` and set it as the initial `tools_enabled` on `DesktopState` (replacing the hardcoded `false` default). The `set_tools_enabled` command (already implemented as task 2.10 of `add-desktop-app`) must also call `config::save()` after updating the in-memory state so the choice persists. Verify round-trip: save `tools.enabled = true`, restart app, confirm the toggle is on.

- [ ] 1.4 **[MVP]** Add `reqwest = { version = "0.12", features = ["json"] }` as a **direct** dependency in `src-tauri/Cargo.toml`. A transitive dep in `Cargo.lock` is not directly usable in `src-tauri`'s own `use reqwest::...`. Also add `tauri-plugin-dialog = "2"` and `tauri-plugin-opener = "2"` to `src-tauri/Cargo.toml`. Run `cargo build` inside `src-tauri/` to confirm all three resolve.

- [ ] 1.5 **[MVP]** Register the new plugins in `src-tauri/src/lib.rs` `tauri::Builder` chain, BEFORE `.setup()`, in this order: existing log plugin, then `.plugin(tauri_plugin_dialog::init())`, then `.plugin(tauri_plugin_opener::init())`. The existing log plugin stays at the top. Confirm `cargo tauri dev` still launches.

- [ ] 1.6 **[MVP]** Update `src-tauri/capabilities/default.json` to add the new permissions:
  - `"dialog:default"` — for the native folder picker (Screen 1 of wizard).
  - `{ "identifier": "opener:allow-open-url", "allow": [{"url": "https://openrouter.ai/*"}, {"url": "https://openai.com/*"}, {"url": "https://platform.openai.com/*"}] }` — for the provider-dashboard "Get a key" links. Use specific domain allowlists, NOT a blanket `https://*` wildcard, to minimize attack surface if the webview is ever compromised.
  - Do NOT add `http:default` or any capability for the `probe_connection` command — that command uses Rust-side `reqwest` directly and is not subject to the Tauri HTTP plugin capability gate.

- [ ] 1.7 **[MVP]** Add a comment above the single `tauri::generate_handler![...]` call in `src-tauri/src/lib.rs`:
  ```rust
  // SINGLE invoke_handler — Tauri keeps only the last registration.
  // Add ALL new commands here. Never call .invoke_handler() a second time.
  ```
  This prevents the silent "all but last registration discarded" footgun as the command surface grows.

- [ ] 1.8 **[MVP]** Migrate `src-tauri/src/commands.rs` to a `src-tauri/src/commands/` directory module. Extract logical groups into sub-modules (keep all functions `pub`):
  - `commands/chat.rs` — `send_message`, `get_history`, `get_status`, `clear_conversation`
  - `commands/settings.rs` — `get_config`, `save_config`, `set_tools_enabled`
  - `commands/onboarding.rs` — `save_onboarding_config`, `get_app_state`, `probe_connection`, `check_path_writable`, `pick_data_dir` (new in phase 2)
  - `commands/memory.rs` — `get_memory_md`, `write_memory_md`, `get_today_note`, `search_memory_files` (new in phase 4)
  - `commands/skills.rs` — `list_skills`, `read_skill`, `create_skill` (new in phase 5)
  - `commands/system.rs` — `run_doctor`, `open_external_url` (new in phase 6)
  - `commands/mod.rs` — re-exports each sub-module.
  Update the single `generate_handler![...]` in `lib.rs` to use module-qualified names: `commands::chat::send_message`, `commands::settings::get_config`, etc. This is a mechanical, behavior-preserving refactor. Verify `cargo build` passes with no behavior change.

- [ ] 1.9 Add `Mutex<open_assistant::core::plugins::PluginMarketplace>` field to `AppCore` / `DesktopState` in `src-tauri/src/state.rs`, initialized in `build_core()` with `PluginMarketplace::new(format!("{}/.claude/plugins", cfg.general.data_dir))` followed by `.load_installed()` (both are sync calls, no `block_on` needed). This enables in-session plugin enable/disable toggling (phase 7). Label the field with `// session-only: enabled state resets on restart until set_enabled() writes back to plugin.json`.

---

## Phase 2 — Onboarding: new Tauri commands

> Implement the backend commands the wizard frontend needs. No UI yet. Each command is independently testable (see phase 7 Rust tests).

- [ ] 2.1 **[MVP]** Implement `#[tauri::command] async fn get_app_state() -> Result<AppStateDto, String>` in `commands/onboarding.rs`. Returns:
  ```rust
  #[derive(Serialize)]
  pub struct AppStateDto {
      pub initial_view: String,   // "onboarding" | "chat"
      pub api_key_set: bool,
      pub user_name: String,
      pub data_dir: String,
  }
  ```
  Logic: load config, if `api_key` is empty → `initial_view = "onboarding"`, else → `"chat"`. The frontend calls this on mount BEFORE first paint to route correctly and avoid a flash of the Chat view before routing to the wizard. Register in `generate_handler!`.

- [ ] 2.2 **[MVP]** Implement `#[tauri::command] async fn probe_connection(api_key: String, api_base: String, model: String) -> Result<ProbeResultDto, String>` in `commands/onboarding.rs`. Use `reqwest::Client` directly (not `Agent::process` — calling the agent here would write daily notes and mutate `UserModel` on the user's very first interaction). Send a minimal `POST {api_base}/chat/completions` with `Authorization: Bearer {api_key}`, `model`, `messages: [{role: "user", content: "hi"}]`, `max_tokens: 1`, timeout 10s. Return:
  ```rust
  #[derive(Serialize)]
  pub struct ProbeResultDto {
      pub ok: bool,
      pub latency_ms: u64,
      pub error_type: Option<String>, // "auth_failure" | "model_unavailable" | "network_error"
      pub error_message: Option<String>,
  }
  ```
  Map outcomes: HTTP 200 → `ok: true`; HTTP 401/403 → `error_type: "auth_failure"`; HTTP 404 → `error_type: "model_unavailable"` (default model may be absent from some providers); timeout/DNS/connection refused → `error_type: "network_error"`. The `model_unavailable` distinction prevents the default `openrouter/owl-alpha` model absence from blocking users with a valid key. Register in `generate_handler!`.

- [ ] 2.3 **[MVP]** Implement `#[tauri::command] async fn check_path_writable(path: String) -> Result<bool, String>` in `commands/onboarding.rs`. Creates the directory (and `memory/` and `skills/` subdirs) via `tokio::fs::create_dir_all` if absent, then tests writability by writing and removing a `.probe` temp file. Returns `Ok(true)` if writable, `Ok(false)` if not (not an `Err`; the frontend shows inline UI for both states). Register in `generate_handler!`.

- [ ] 2.4 **[MVP]** Implement `#[tauri::command] async fn pick_data_dir(app: tauri::AppHandle) -> Result<Option<String>, String>` in `commands/onboarding.rs`. Call `tauri_plugin_dialog::DialogExt::dialog(&app).file().blocking_pick_folder()` inside `tauri::async_runtime::spawn_blocking` to avoid blocking the Tokio executor. Convert the returned `tauri_plugin_dialog::FilePath` via `.as_path().map(|p| p.display().to_string())` — do NOT call `.to_string()` directly on the `FilePath` enum (it serializes the enum variant, not the path string). On Windows, `Path::display()` produces OS-native backslashes. Register in `generate_handler!`.

- [ ] 2.5 **[MVP]** Implement `#[tauri::command] async fn save_onboarding_config(dto: OnboardingDto) -> Result<(), String>` in `commands/onboarding.rs`. Define:
  ```rust
  #[derive(Deserialize)]
  pub struct OnboardingDto {
      pub data_dir: String,
      pub provider: String,
      pub model: String,
      pub api_base: String,
      pub api_key: String,           // never empty when called from wizard Screen 2
      pub tools_enabled: bool,
      pub user_name: Option<String>, // None → use "friend" default
      pub skills_dirs: Vec<String>,  // wizard always sends at least [data_dir + "/skills"]
  }
  ```
  Use the full load/mutate/save pattern (NEVER `config::set()`):
  ```rust
  let mut cfg = open_assistant::config::load().await.map_err(|e| e.to_string())?;
  cfg.general.data_dir = dto.data_dir;
  cfg.model.provider = dto.provider;
  cfg.model.model = dto.model;
  cfg.model.api_base = dto.api_base;
  if !dto.api_key.is_empty() { cfg.model.api_key = dto.api_key; }
  cfg.tools.enabled = dto.tools_enabled;
  cfg.general.user_name = dto.user_name.unwrap_or_else(|| "friend".into());
  cfg.skills.dirs = dto.skills_dirs; // Vec<String>, not a comma-separated string
  open_assistant::config::save(&cfg).await.map_err(|e| e.to_string())
  ```
  After saving, update the in-memory `DesktopState.tools_enabled` to match `dto.tools_enabled` (lock the turn mutex briefly). Register in `generate_handler!`.

- [ ] 2.6 **[MVP]** Implement `#[tauri::command] async fn open_external_url(url: String, app: tauri::AppHandle) -> Result<(), String>` in `commands/system.rs`. Use `tauri_plugin_opener::OpenerExt::opener(&app).open_url(&url, None::<&str>).map_err(|e| e.to_string())`. Only called for the provider-dashboard links in wizard Screen 2; the frontend must only pass URLs matching the capability allowlist (openrouter.ai, openai.com, platform.openai.com). Register in `generate_handler!`.

---

## Phase 3 — Onboarding: wizard frontend

> Build the 4-screen wizard overlay in the existing `frontend/` SPA. No new Tauri window — the wizard is an in-app overlay over the `main` window. The existing nav rail is visible but grayed out (aria-disabled) behind the wizard card.

- [ ] 3.1 **[MVP]** Add an `onboarding` view module to `frontend/app.js`. On app mount, call `invoke('get_app_state')` BEFORE rendering any other view. If `initial_view === 'onboarding'`, show the wizard overlay; else proceed to Chat (existing behavior). This replaces the existing api-key-only gate (task 4.2 of `add-desktop-app`) with a superset that handles both "first run" and "key missing" states through one code path. Add `data-testid="onboard-wizard"` to the wizard container.

- [ ] 3.2 **[MVP]** Build the wizard shell: a full-screen overlay (`position: fixed; inset: 0; background: rgba(0,0,0,0.4); z-index: 1000`), a centered card (`max-width: 620px; padding: 2rem; background: white; border-radius: 12px; border: 1px solid var(--border); box-shadow: 0 8px 32px rgba(0,0,0,0.12)`), a step progress bar at the top of the card, Back/Continue buttons at the bottom right, and a Skip link (top-right, where available). The overlay is NOT dismissible by clicking outside — the user must complete or skip to Screen 4. Add `data-testid="onboard-step-bar"`.

- [ ] 3.3 **[MVP]** Implement step progress bar: four filled circles (primary `#2563eb`) with labels: `1 Workspace · 2 AI Provider · 3 Permissions · 4 Finish`. Current step has an animated ring; past steps are filled; future steps are grey `#e2e8f0`. Step labels use `font-size: 0.75rem; color: var(--text-muted)`.

- [ ] 3.4 **[MVP]** Implement **Screen 1 — Workspace**: title "Where should I keep your data?", body copy "openAssistant stores your memory, conversations, and settings here. Your data stays local and private." Show the OS default path (`~/.openassistant` on macOS/Linux, `%USERPROFILE%\.openassistant` on Windows — derive from `get_app_state().data_dir`) as a styled path chip. Include a "Change folder" button that calls `invoke('pick_data_dir')` and updates the chip. Below the chip, show an async writability badge: on mount call `invoke('check_path_writable', { path })` and show green "Writable" or red "Cannot write — choose another folder". Continue is disabled while the check is pending or fails. Add `data-testid` attributes: `onboard-workspace-path`, `onboard-workspace-change`, `onboard-workspace-writable`.

- [ ] 3.5 **[MVP]** Implement **Screen 2 — AI Provider**: title "Connect your AI." Three provider radio cards: OpenRouter (recommended, pre-selected), OpenAI, Custom. Selecting OpenRouter pre-fills `api_base` with `https://openrouter.ai/api/v1`; OpenAI pre-fills `https://api.openai.com/v1`; Custom shows an editable `api_base` text field. Model text field pre-filled per provider (OpenRouter: `openrouter/owl-alpha`, OpenAI: `gpt-4o`, Custom: empty). API key password input with show/hide eye toggle. A "Get an API key" link that calls `invoke('open_external_url', { url })` (does NOT open in the webview — CSP intact). "Test connection" button: on click show spinner/`"Testing..."`, call `invoke('probe_connection', { apiKey, apiBase, model })`, handle all four outcomes:
  - `ok: true` → green banner "Connected! Responded in {latency_ms}ms." Continue unlocks.
  - `error_type: "auth_failure"` → red inline "Invalid API key. Check the value and try again."
  - `error_type: "model_unavailable"` → amber inline "Model not found. Try a different model name, or your key may not have access."
  - `error_type: "network_error"` → amber inline "Could not reach the endpoint. Check the URL and your network."
  Editing `api_key`, `api_base`, or `model` after a passing test clears the result and re-locks Continue. Add `data-testid` attributes: `onboard-provider-radio`, `onboard-api-base`, `onboard-api-key`, `onboard-model`, `onboard-test-connection`, `onboard-connection-result`.

- [ ] 3.6 **[MVP]** Implement **Screen 3 — Tools and Permissions**: title "How much access should openAssistant have?". Two option cards side-by-side:
  - Card A (left, pre-selected default): lock icon, headline "Chat only", description "No file or shell access. Safer for most users."
  - Card B (right): terminal icon, headline "Enable tools", description "The assistant can run shell commands and read/write files. You'll confirm each command."
  The Continue button is disabled until one card is clicked (user MUST make an explicit choice). If Card B is selected, show an amber soft-banner: "You can change this any time in Settings. The assistant will ask before each command." Add `data-testid` attributes: `onboard-tools-card-a`, `onboard-tools-card-b`, `onboard-tools-consent`.

- [ ] 3.7 **[MVP]** Implement **Screen 4 — Identity and Finish**: title "One last thing — what should I call you?". Single text input, placeholder "friend", max 64 chars, trimmed. Skip link top-right that advances without saving a name. Three async status pills that update in parallel on mount:
  - "Config saved" — checks immediately (calls `get_app_state` to confirm `api_key_set`).
  - "Data directory ready" — already validated in Screen 1; always green here.
  - "Vision tools" — calls `invoke('run_doctor')` filtered to the vision check; shows green "Gemini CLI detected" or amber "Not found — image analysis unavailable". If the command is still in flight after 200ms, render amber "Checking..." → resolves when done. The Finish CTA ("Start chatting →") is NOT blocked by pill states.
  On "Start chatting →" click: call `invoke('save_onboarding_config', { ...allFields, userName })`, dismiss the overlay, navigate to Chat. Add `data-testid` attributes: `onboard-username`, `onboard-pill-config`, `onboard-pill-datadir`, `onboard-pill-vision`, `onboard-finish-cta`.

- [ ] 3.8 **[MVP]** Implement wizard state machine: Back button always restores all field values (no data loss on back-navigation). Connection test result (Screen 2) is invalidated if `api_key`, `api_base`, or `model` changed after going back. All screens retain their values in JS state (not re-fetched from the backend on each visit). Wizard state is fully cleared when the wizard overlay is dismissed.

- [ ] 3.9 **[MVP]** Implement wizard re-entry points:
  - Settings view: add a "Re-run Setup Wizard" text button below the Save button (`data-testid="settings-rerun-wizard"`). Clicking it shows the wizard overlay in re-entry mode: all fields pre-filled from `get_config()`, `api_key` shown as `"••••...••••"` with a "Change" button that clears and re-locks the field. The connection test result is cleared; user must re-run the test if they change any field.
  - Status/Doctor panel: if `api_key_set === false` or any Doctor check fails, show a "Run Setup Wizard" action link (`data-testid="status-rerun-wizard"`).
  - The existing launch gate (now replaced by the `get_app_state` flow in task 3.1) no longer routes directly to Settings — it routes to the full wizard overlay.

- [ ] 3.10 Extend the existing `defaultMock` / `window.__MOCK_BACKEND__` in `frontend/app.js` with mock implementations for all new wizard commands: `get_app_state` (returns `{ initial_view: "chat", api_key_set: true, user_name: "Test User", data_dir: "/tmp/test" }`), `probe_connection` (returns `{ ok: true, latency_ms: 42 }`), `check_path_writable` (returns `true`), `pick_data_dir` (returns `"/tmp/test-picked"`), `save_onboarding_config` (returns `null`), `open_external_url` (returns `null`). Extend also the Playwright `installMock` in `tests/e2e/` to mirror these.

---

## Phase 4 — Expanded Settings

> Refactor the existing single-card Settings view into a two-column sectioned panel. All writes use load/mutate/save (never `config::set()`).

- [ ] 4.1 **[MVP]** Expand `get_config` in `commands/settings.rs` to return a richer `ConfigDto`:
  ```rust
  pub struct ConfigDto {
      // Model (already exposed)
      pub provider: String, pub model: String, pub api_base: String,
      pub api_key_masked: String, pub api_key_set: bool,
      // General (new)
      pub user_name: String, pub data_dir: String, pub log_level: String,
      // Tools (new)
      pub tools_enabled: bool,
      // Memory (new)
      pub memory_max_entries: i64, pub memory_fts_enabled: bool,
      // Skills (new)
      pub skills_dirs: Vec<String>, pub skills_auto_create: bool,
      // Channels (new — gateway tokens, masked)
      pub discord_token_set: bool, pub telegram_token_set: bool, pub slack_token_set: bool,
      // Security (new)
      pub dm_pairing: bool,
      // Vision (new)
      pub vision_provider: String, pub vision_gemini_path: String,
      // App meta
      pub app_version: String,  // read from CARGO_PKG_VERSION at compile time
  }
  ```

- [ ] 4.2 **[MVP]** Expand `save_config` in `commands/settings.rs` to accept all `ConfigDto` fields (gateway tokens as `Option<String>` — only overwrite if `Some` and non-empty, to avoid wiping a token when the masked field is not changed). Use full load/mutate/save. After saving, update in-memory `DesktopState.tools_enabled` if the tools section changed.

- [ ] 4.3 **[MVP]** Rebuild the Settings view as a two-column layout: a secondary left category sidebar (140px, `background: var(--bg-secondary); border-right: 1px solid var(--border)`) with 7 category items, and a right content panel. Active category is highlighted with `background: var(--primary); color: white; border-radius: 6px`. Add `data-testid="settings-nav"`.

- [ ] 4.4 **[MVP]** Implement Settings sections (right panel content swaps on category click):
  - **Model** — provider radio, model text field, api_base text field, masked api_key input with show/hide. Test connection button (same `probe_connection` as wizard). `data-testid="settings-model"`, `settings-api-base"`, `settings-api-key"`.
  - **General** — user_name text field, data_dir read-only chip with "Open folder" button (calls `open_external_url` to the folder URI), log_level dropdown (debug/info/warn/error).
  - **Tools** — `tools_enabled` toggle (already implemented, keep). Warning copy: "Enables bash, file read/write, and edit tools. The assistant will ask before each command." Default OFF. `data-testid="settings-tools-toggle"`.
  - **Memory** — max_entries number field, fts_enabled toggle, db_path display (read-only). `data-testid="settings-memory"`.
  - **Skills** — skills_dirs list (each dir shown as a chip with a remove button; "Add directory" input), auto_create toggle. `data-testid="settings-skills-dirs"`.
  - **Channels** — discord_token, telegram_token, slack_token password inputs (show/hide), dm_pairing toggle. A visible callout banner: "Gateway channels are experimental — the messaging server is not yet fully operational." `data-testid="settings-channels"`.
  - **Advanced** — vision_provider dropdown (gemini_cli/none), gemini_path text field. A "Re-run Setup Wizard" text button. Settings footer showing app version only — NO Update button (the CLI update command just prints cargo instructions and MUST NOT be surfaced as a working button). `data-testid="settings-advanced"`, `settings-app-version"`, `settings-rerun-wizard"`.

- [ ] 4.5 Add a Save button (primary style) and a success/error toast for each Settings section save. The Save button is per-section (saves only the active section's fields), not a global save-all. Each section save calls `save_config` with the full `ConfigDto` (other sections' values are read from `get_config()` to avoid overwriting them).

---

## Phase 5 — Expanded `get_status` and `run_doctor` commands

- [ ] 5.1 **[MVP]** Expand `get_status` in `commands/chat.rs` to return additional fields:
  - `memory_db_entries: i64` — via `open_assistant::memory::store::MemoryStore::open_default().await?.count()`.
  - `memory_md_chars: usize` — via `open_assistant::core::memory::MemoryWorkspace::from_data_dir(&cfg.general.data_dir).read_long_term().len()`.
  - `data_dir: String` — from config.
  Both calls are non-blocking I/O already used in the CLI status handler (`src/main.rs:102-113`). Update `StatusResponse` struct and update the `defaultMock` in the frontend to include these fields with realistic values.

- [ ] 5.2 **[MVP]** Implement `#[tauri::command] async fn run_doctor(app_state: tauri::State<'_, DesktopState>) -> Result<Vec<DiagnosticResultDto>, String>` in `commands/system.rs`:
  ```rust
  #[derive(Serialize)]
  pub struct DiagnosticResultDto {
      pub name: String,
      pub ok: bool,
      pub message: String,
      pub is_optional: bool,  // amber (not red) when optional + failing
  }
  ```
  Run all six diagnostic checks from `src/main.rs:304-343` in sequence, collecting each result. Critical implementation notes:
  - **Skills check**: use `open_assistant::skills::engine::SkillEngine::load_builtin()?.count()` — returns 3 (the real built-in count). Do NOT use `SkillEngine::default().count()` which returns 0 because `default()` does not call `load_builtin()`. This is a known CLI bug.
  - **Vision check**: wrap `open_assistant::tools::vision::check()` in a `tokio::time::timeout(Duration::from_millis(200), ...)`. If it times out or returns `Err` (e.g., `gemini` not on PATH on Windows), return `ok: false, is_optional: true, message: "Gemini CLI not found — image analysis unavailable"`. Mark as amber/optional, not a red error.
  - **Gateway check**: mark as `is_optional: true`; failure means tokens not configured, which is expected for most users.
  - All other checks (`config::check`, SQLite open, workspace init) are non-optional.
  Register in `generate_handler!`. Add `run_doctor` to the Playwright `installMock`.

---

## Phase 6 — Real feature panels: Memory browser, Skills manager, Status/Doctor

> Wire the three currently-disabled nav items (Memory, Skills, and the renamed Status item). Each requires new Tauri commands from tasks below.

- [ ] 6.1 **[MVP]** Rename the "Integrations" nav item to "Status" in `frontend/index.html` and `frontend/app.js`. Update `data-testid="nav-integrations"` → `data-testid="nav-status"`. This reflects that gateway/OAuth integrations are not ready; the Status/Doctor panel is what ships here. Update any existing Playwright test selectors.

- [ ] 6.2 **[MVP]** Implement memory Tauri commands in `commands/memory.rs` (all use `MemoryWorkspace::from_data_dir(&cfg.general.data_dir)` — load config once at the top of each command):
  - `get_memory_md() -> Result<String, String>` — calls `ws.read_long_term()`.
  - `write_memory_md(content: String) -> Result<(), String>` — calls `ws.write_long_term(&content)`.
  - `get_today_note() -> Result<String, String>` — calls `ws.read_today()`.
  - `search_memory_files(query: String) -> Result<Vec<[String; 2]>, String>` — calls `ws.search_files(&query)`, returns `[[filename, excerpt], ...]`.
  All use the real `MemoryWorkspace` methods from `src/core/memory.rs`. Register all four in `generate_handler!`.

- [ ] 6.3 **[MVP]** Implement skills Tauri commands in `commands/skills.rs`:
  - `list_skills() -> Result<Vec<SkillDto>, String>` — call `SkillEngine::load_builtin()` then `load_from_dir(&cfg.skills.dirs[0])` if present; return `name`, `description`, `category`, `is_builtin` per skill.
  - `read_skill(name: String) -> Result<String, String>` — return skill `content` from engine.
  - `create_skill(name: String, content: String) -> Result<(), String>` — `tokio::fs::write(format!("{}/skills/{}.md", cfg.skills.dirs[0], name), content)`. Do NOT expose `activate_skill()` — it is not wired into `Agent::process` and would have no effect.
  Register all three in `generate_handler!`.

- [ ] 6.4 Build the **Memory** view (wire the Memory nav item, remove "soon" badge):
  - Left pane (280px): file list showing `MEMORY.md`, `DREAMS.md`, and `memory/YYYY-MM-DD.md` daily notes from `search_memory_files("")`. A search box that calls `search_memory_files(query)` on input with 300ms debounce. `data-testid="memory-list"`, `memory-search"`.
  - Right pane: a `<textarea>` showing the selected file's content. `MEMORY.md` is editable (Save button calls `write_memory_md`); daily notes are read-only. `data-testid="memory-content"`, `memory-save"`.
  - On Memory nav click, call `get_memory_md()` and `get_today_note()` to hydrate both panes.

- [ ] 6.5 Build the **Skills** view (wire the Skills nav item, remove "soon" badge):
  - Left pane: skill list from `list_skills()`, grouped into "Built-in" and "Custom" sections. A "New Skill" button opens a modal with name + content textarea. `data-testid="skills-list"`, `skills-new"`.
  - Right pane: selected skill's content in a read-only `<pre>` block with name, description, and category. No "Activate" button. A note: "Skill activation affects agent behavior and will be available in a future update." `data-testid="skills-content"`.

- [ ] 6.6 Build the **Status/Doctor** view (wire the Status nav item, remove "soon" badge):
  - **Status card**: model, api_base, workspace, message_count, memory_db_entries, memory_md_chars from expanded `get_status()`. `data-testid="status-card"`.
  - **Doctor card**: a "Run Diagnostics" button that calls `run_doctor()` and renders each `DiagnosticResultDto` as a colored row: green check (ok), red X (not ok + not optional), amber triangle (not ok + is_optional). `data-testid="doctor-card"`, `doctor-run"`, `doctor-results"`.
  - If `api_key_set === false`: show a "Run Setup Wizard" action link below the status card (`data-testid="status-rerun-wizard"`).
  - Read-only agents definitions sub-section: call a new `list_agents()` command (task 6.7), show `name`, `description`, `tools` from `.claude/agents/*.md`. No "Run" button. A note: "Agent execution is not yet available in the desktop app." `data-testid="agents-list"`.

- [ ] 6.7 Implement `#[tauri::command] async fn list_agents() -> Result<Vec<AgentDto>, String>` in `commands/system.rs` using `SubAgentOrchestrator::load_definitions(&format!("{}/.claude/agents", cfg.general.data_dir))` and `list_definitions()`. Return `name`, `description`, `tools`, `model` per definition. Do NOT add a `spawn_agent` command — `execute_subagent()` returns a hardcoded placeholder regardless of the model or goal (`src/core/subagent.rs:267-283`). Register in `generate_handler!`.

---

## Phase 7 — Tests

> First comprehensive test suite for the new command layer, wizard flow, and real panel commands.

- [ ] 7.1 **[MVP]** Add Rust `#[cfg(test)]` test for `save_onboarding_config` round-trip in `src-tauri/src/commands/onboarding.rs` (or a `tests/` integration test): write all wizard fields via the full load/mutate/save path, reload config with `config::load()`, assert `data_dir`, `user_name`, `provider`, `model`, `api_base`, `skills_dirs` all persisted, `api_key` is stored (not masked in the raw config), and `tools.enabled` persisted. Run on all platforms in CI.

- [ ] 7.2 **[MVP]** Add a Rust test for `probe_connection` error mapping: mock a local HTTP server (use `wiremock` or `httpmock`) returning 401, 404, 200, and connection-refused cases; assert `ProbeResultDto.error_type` maps correctly for each. Specifically assert that a 404 produces `"model_unavailable"` (not `"auth_failure"`) so the default-model-absence case is distinguishable.

- [ ] 7.3 **[MVP]** Add a Rust test for `get_status` extensions: assert `memory_db_entries` and `memory_md_chars` are present in the returned struct and are non-negative integers (use a temp data dir with an empty `MEMORY.md` and a fresh `memory.db`).

- [ ] 7.4 **[MVP]** Add a Rust test for `run_doctor` structure: assert the returned `Vec<DiagnosticResultDto>` has exactly 6 elements, that the skills check shows `ok: true` with count ≥ 3 (not 0), and that the vision check is marked `is_optional: true` when gemini is absent (mock the subprocess to return `Err`).

- [ ] 7.5 **[MVP]** Extend the Playwright `installMock` in `tests/e2e/` with mock implementations for all new commands. Each mock must return a realistic payload matching the actual `Serialize`d Rust struct shape. Include three probe_connection mock states: success, auth_failure, and network_error (so the Playwright wizard test can exercise all three Screen 2 outcomes).

- [ ] 7.6 **[MVP]** Add a Playwright test covering the full wizard happy path: app opens in onboarding state (mock `get_app_state` returns `initial_view: "onboarding"`), user steps through all 4 screens, fills api_key, runs probe (mock success), selects Card A (tools off), fills name, clicks "Start chatting →". Assert: wizard overlay dismissed, Chat view rendered, `save_onboarding_config` was called once with the correct field values.

- [ ] 7.7 Add a Playwright test covering wizard re-entry from Settings: navigate to Settings > Advanced, click "Re-run Setup Wizard", assert wizard overlay shows with pre-filled fields, api_key shown as masked, connection test is cleared (Continue locked until re-tested).

- [ ] 7.8 Add Playwright tests for Memory view: navigate to Memory, assert `memory-content` renders the mocked MEMORY.md text, edit textarea, click Save, assert `write_memory_md` was called with new content. Assert daily note pane is read-only (no Save button when a daily note is selected).

- [ ] 7.9 Add Playwright tests for Skills view: navigate to Skills, assert built-in skills appear in list, click a skill, assert `read_skill` was called and content renders. Click "New Skill", fill form, submit, assert `create_skill` called. Assert no "Activate" button is visible.

- [ ] 7.10 Add Playwright tests for Status/Doctor view: navigate to Status, assert status card shows model + message_count + memory_db_entries. Click "Run Diagnostics", assert `run_doctor` called, assert result rows render (6 rows). If `api_key_set: false` in mock, assert "Run Setup Wizard" link is visible.

---

## Phase 8 — Documentation and final acceptance

- [ ] 8.1 **[MVP]** Update `CLAUDE.md` Build & Run section: add `cargo tauri dev` (desktop dev mode), note that `tauri-plugin-dialog` and `tauri-plugin-opener` are now in use, note the `commands/` module structure, and document the single `generate_handler!` invariant.

- [ ] 8.2 **[MVP]** Add a "What is NOT available in the desktop app (this phase)" section to the change's notes / commit message body, explicitly listing every item from the scope guardrail table (sub-agent execution, workflow execution, checkpoint restore, marketplace install, self-update, skill activation, live gateway dashboard, goal/plan/perm handlers). This ships with the PR.

- [ ] 8.3 **[MVP]** Final acceptance gate: on a clean Windows machine with a valid OpenRouter key, `cargo tauri build` → install → launch → app routes to wizard overlay → user steps through 4 screens → saves config → Chat view opens → sends a real message and renders the actual LLM reply. Confirm Memory view shows `MEMORY.md`. Confirm Skills view lists ≥ 3 built-in skills. Confirm Status/Doctor runs all 6 checks. Confirm the stub features (Agents, Workflows, Checkpoints, Update) are absent from the UI.

- [ ] 8.4 Update `openspec/project.md` to note that the "Status" nav item (renamed from "Integrations") surfaces real diagnostics and that the wizard is re-enterable from Settings. Remove the "Integrations" nav item reference and replace with "Status".

- [ ] 8.5 Verify `cargo build` (root crate), `cargo run -- tui`, `cargo run -- status`, and `cargo run -- web --port 3000` still pass (CLI not regressed by the `Config::tools` field addition in phase 1). The new `[tools]` section in `config.yaml` must have a proper `Default` impl so existing configs that lack the field deserialize without error (serde `#[serde(default)]` on the field in `Config`).
