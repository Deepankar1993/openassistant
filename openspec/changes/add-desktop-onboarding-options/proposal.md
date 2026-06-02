# Add Desktop Onboarding & Options

## Why

The `add-desktop-app` change delivered a working Tauri 2.x shell: a wired non-streaming Chat backed by `Agent::process`, a minimal Settings view (model / api\_base / masked api\_key + tools toggle), and an API-key gate that redirects to Settings on first run. That MVP proves the architecture; this change finishes the first-run experience and surfaces every terminal capability that has a real, non-stub core implementation.

Three concrete problems motivate this change:

1. **The first-run gate is a dead-end.** When `api_key` is empty the app lands on the Settings form with no contextual help — no explanation of which provider to use, no way to test the connection, no guided data-dir setup, and no tools-consent choice that persists across restarts. The 6-step stdin wizard in `src/onboarding/wizard.rs` has all the right questions but cannot run inside a Tauri WebView (it blocks on `io::stdin()`). A proper GUI wizard is the natural successor.

2. **The tools-consent choice is not persisted.** `set_tools_enabled` (tasks.md 2.10) flips `Turn.agent.tools_enabled` in memory only. There is no `tools` section in `src/config/mod.rs`, so every restart silently resets the user's decision. The wizard's Permissions screen is meaningless unless this is fixed.

3. **Real desktop panels exist for capabilities that are already wired end-to-end.** Memory file I/O (`MemoryWorkspace`), Skills listing/creation (`SkillEngine`), and the Doctor diagnostics (`src/main.rs:304-343`) are all real, non-stub code paths — but the nav items for Memory and Skills remain disabled placeholders, and there is no desktop equivalent of `cargo run -- doctor` or `cargo run -- status` beyond the tiny Chat sidebar. Surfacing these in real panels closes the gap between the desktop app and the terminal for everything the core actually supports.

The `add-desktop-app` change documented which core features are stubs (sub-agents, workflows, checkpoints, gateway, self-update). This change does **not** re-litigate those; it adds real surfaces only for the features whose cores are verified functional.

## What Changes

The following items are the reconciled MVP for this change. Items that appeared in the research digest but were cut by the expert panel are in the Non-Goals section below.

### 1. Persist `tools_enabled` to `config.yaml`

Add a `[tools]` section to `src/config/mod.rs` with a single `enabled: bool` field (default `false`). Update `build_core()` in `src-tauri/src/lib.rs` to read `config.tools.enabled` when constructing `AppCore` so the agent starts in the correct posture. Update `set_tools_enabled` to call `config::load() → mutate tools.enabled → config::save()` (the same load/mutate/save pattern as `save_config`; never route through `config::set()`). This is a prerequisite for the onboarding wizard's Permissions screen to be meaningful.

### 2. Onboarding wizard (4-screen GUI, replaces the Settings redirect)

Implement as a full-screen overlay rendered in the existing `main` window via SPA routing — no second `WebviewWindowBuilder` (avoids the Windows WebView2 synchronous-window-creation deadlock and the need for a second capability entry). The frontend detects `onboarding_complete === false` on mount and renders the wizard overlay over the grayed-out nav rail.

**Screens:**

- **Screen 1 — Workspace.** Pre-fill `general.data_dir` with the OS default (`%USERPROFILE%\.openassistant` on Windows, `~/.openassistant` elsewhere) shown as a styled path chip with a "Change folder" button. The button calls a new `pick_data_dir` Tauri command backed by `tauri-plugin-dialog` `blocking_pick_folder()` (wrapped in `tauri::async_runtime::spawn_blocking`; convert the returned `FilePath` via `.as_path().map(|p| p.display().to_string())`). An async writability badge (green "Writable" / red "Cannot write to this folder") is driven by a `check_path_writable` Tauri command; Continue is disabled until the badge resolves green. On advance, `check_path_writable` creates the directory and `skills/` / `memory/` subdirs. Display the path with OS-native separators (backslash on Windows).

- **Screen 2 — AI Provider.** Three radio-card options: OpenRouter (recommended, pre-selects `api_base = https://openrouter.ai/api/v1`, `model = openrouter/owl-alpha`), OpenAI (pre-selects `api_base = https://api.openai.com/v1`, `model = gpt-4o`), Custom (reveals editable `api_base` field). A masked `api_key` password input with show/hide toggle and a "Get an OpenRouter key →" link that calls `tauri-plugin-opener` `open_url()` (not a WebView navigation — keeps CSP intact). A "Test connection" button below the key field fires a new `probe_connection` Tauri command that sends a minimal `max_tokens=1` POST to `{api_base}/chat/completions` with Bearer auth, implemented as a direct `reqwest` call (added as a direct `src-tauri/Cargo.toml` dependency; not through `tauri-plugin-http`, which only gates frontend JS fetch). The probe returns one of three outcomes: success with latency, auth failure (401/403), or network failure (timeout / 5xx / DNS). Continue is locked until a passing test; editing `api_key`, `api_base`, or provider clears the cached result. The probe MUST NOT call `Agent::process` — that would write a daily-note entry before the user has ever chatted.

- **Screen 3 — Permissions (non-skippable).** Title: "How much access should openAssistant have?" Two large option cards: Card A (default, pre-selected) "Chat only — no shell or file access"; Card B "Enable tools — I understand the assistant can run commands and read/write files." Continue is disabled until one card is clicked. If Card B is selected, show an amber notice: "You can change this any time in Settings." Writes `tools_enabled` to the persisted config field added in item 1.

- **Screen 4 — Identity + Finish.** Optional text input "What should I call you?" (placeholder `friend`, max 64 chars, trimmed; default `friend` if blank). Three async status pills shown below the input that update as each resolves: "Config saved", "Data directory ready", "Vision tools" (calls `tools::vision::check()` with a 200ms timeout, renders green "Gemini CLI detected" or amber "Not found — image analysis unavailable"; absent binary on Windows must not produce a red error). Large "Start chatting →" CTA saves `general.user_name` and navigates to Chat. Small footnote: "Re-run this setup anytime from Settings."

**Step progress bar** at the top of the wizard card: four filled/outlined circles labeled "Workspace · AI Provider · Permissions · Finish".

**Wizard shell:** a card (max-width 620px, 12px border-radius, `box-shadow`, white background, `#e2e8f0` border) centered on the full-screen overlay. The nav rail is visible but `aria-disabled`. Clicking outside the card does nothing. Back navigation retains all field values; the connection-test result is invalidated if `api_key`, `api_base`, or provider changed.

**Onboarding-complete detection.** Add an `onboarding_complete: bool` field to the `get_config` response DTO (or expose a separate `get_app_state` command), derived from `!config.model.api_key.is_empty()`. The frontend checks this before first paint to avoid flashing Chat. The existing API-key gate (task 4.2) is reconciled: a fresh install (no key, no prior config) routes to the wizard overlay; an existing install with an empty key can show the wizard or route to Settings with a "Re-run Setup Wizard" link — both paths share the same `save_onboarding_config` command.

**Re-entry points:** (a) Settings view: "Re-run Setup Wizard" text button below Save; (b) Status/Doctor panel: "Run Setup Wizard" link when `api_key` is empty; (c) the launch gate already in task 4.2. When re-running, all wizard fields pre-fill from the existing config with the api\_key masked as `••••...••••`; a "Change" affordance clears the field and requires a new passing connection test.

**New Tauri commands for the wizard:**
- `probe_connection(api_key: String, api_base: String, model: String) -> Result<ProbeResult, String>` — direct `reqwest` POST, returns `{ ok: bool, latency_ms: u32, error_type: Option<String> }`.
- `check_path_writable(path: String) -> Result<bool, String>` — creates dir and subdirs if absent, checks write permission.
- `pick_data_dir(app: AppHandle) -> Result<Option<String>, String>` — wraps `blocking_pick_folder()` in `spawn_blocking`, converts `FilePath` via `.as_path()`.
- `save_onboarding_config(dto: OnboardingDto) -> Result<(), String>` — accepts all wizard fields (data\_dir, user\_name, provider, model, api\_key, api\_base, tools\_enabled), uses load/mutate/save to write all at once atomically.

### 3. Settings view expansion (sectioned layout)

Refactor the current single flat-form Settings card into a two-column layout (120px category sidebar + content panel) with the following sections. This replaces the current one-card design without removing any existing field.

- **Model** — existing fields (model, api\_base, masked api\_key).
- **General** — `general.user_name` (text input), `general.data_dir` (read-only path chip with "Open folder" button via `tauri-plugin-opener`), `general.log_level` (dropdown: error / warn / info / debug).
- **Tools** — existing `tools_enabled` toggle (task 4.4) moved here as its own section with the consent copy.
- **Memory** — `memory.max_entries` (number), `memory.fts_enabled` (toggle), `memory.db_path` (read-only display).
- **Skills** — `skills.dirs` (editable list), `skills.auto_create` (toggle).
- **Channels** — `gateway.discord_token`, `gateway.telegram_token`, `gateway.slack_token` (all masked, same masking pattern as api\_key). A visible callout banner: "Gateway integration is experimental — the messaging server is not yet fully operational." Tokens are saved to `config.yaml` via the same load/mutate/save path.
- **About** — app version string (read-only), "Re-run Setup Wizard" text button.

`ConfigDto` is extended to carry the new fields. `save_config` continues to use the load/mutate/save pattern, never `config::set()`. Gateway token fields are masked in the DTO exactly as `api_key` is today.

### 4. Memory browser panel (enables the disabled nav item)

Enable the Memory nav rail item. Implement a two-pane panel:

- **Left pane:** list of memory entries — `MEMORY.md`, today's daily note, and a search box wired to `MemoryWorkspace::search_files`.
- **Right pane:** a `<textarea>` showing the selected file's content. `MEMORY.md` is editable (Save button calls `write_memory_md`). Daily notes are read-only. An "Append" button below the editor calls `append_memory`.

New Tauri commands:
- `get_memory_md() -> Result<String, String>`
- `write_memory_md(content: String) -> Result<(), String>`
- `get_today_note() -> Result<String, String>`
- `append_memory(content: String) -> Result<(), String>`
- `search_memory_files(query: String) -> Result<Vec<[String; 2]>, String>`

All delegate to `MemoryWorkspace` methods in `src/core/memory.rs`. `MemoryWorkspace::from_data_dir` takes `config.general.data_dir` from the loaded config.

### 5. Skills manager panel (enables the disabled nav item)

Enable the Skills nav rail item. Implement a two-pane panel:

- **Left pane:** list of skills loaded via `SkillEngine::load_builtin()` (the 3 built-ins) plus `load_from_dir` against `config.skills.dirs[0]`. Each entry shows name, category, and a "(built-in)" badge where applicable.
- **Right pane:** read-only markdown display of the selected skill's content. A "New Skill" button opens a modal with name and content inputs; saving calls `create_skill` which writes a `.md` file to `config.skills.dirs[0]` via `tokio::fs::write`.

Do NOT expose a skill activation toggle — `activate_skill()` is not wired into `Agent::process` and has no observable effect (noted risk: skills activation not connected to the agent loop).

New Tauri commands:
- `list_skills() -> Result<Vec<SkillDto>, String>` — `SkillDto { name, description, category, builtin: bool }`.
- `read_skill(name: String) -> Result<String, String>` — returns the skill file content.
- `create_skill(name: String, content: String) -> Result<(), String>` — writes to the first skills dir.

### 6. Status / Doctor panel (new nav item or Settings sub-section)

Replace the "Integrations" nav item (currently disabled, the label implies OAuth connect flows that do not exist) with "Status". Enable it with a single-panel layout:

- **Status card:** model, api\_base, workspace dir, message count, tools\_enabled (from the extended `get_status` response). Add `memory_db_entries` (via `MemoryStore::open_default().await?.count()`) and `memory_md_chars` (`MemoryWorkspace::from_data_dir(...).read_long_term().len()`) to `StatusResponse`.

- **Doctor card:** a "Run diagnostics" button that fires a new `run_doctor() -> Result<Vec<DiagnosticResult>, String>` command. `DiagnosticResult = { name: String, ok: bool, message: String }`. The command replicates the 6 checks from `src/main.rs:304-343` and returns structured results for colored pass/fail rows:
  1. Config loads cleanly (`config::load()`).
  2. SQLite memory DB opens and schema initialises (`MemoryStore::open_default().await`).
  3. Workspace dirs and `MEMORY.md` exist/are writable (`MemoryWorkspace::from_data_dir().init()`).
  4. Skills count — use `SkillEngine::load_builtin()?.count()` (NOT `SkillEngine::default().count()` which always returns 0).
  5. Gateway config — `gateway::check()`, rendered as informational (not a blocking failure) because gateway is experimental.
  6. Vision / Gemini CLI — `tools::vision::check()` with a 200ms timeout; absent binary rendered as amber "Not found — optional" rather than red error (vision is optional, especially on Windows).

- **Re-run Setup Wizard** action link shown when `api_key` is empty.

Rename the nav item from "Integrations" to "Status". The gateway token fields move to Settings > Channels (item 3 above); there is no top-level Integrations panel this phase.

### 7. `AppCore` state additions

Add to `AppCore` in `src-tauri/src/state.rs`:
- `plugins: Mutex<PluginMarketplace>` — initialized in `build_core()` via `PluginMarketplace::new(format!("{}/.claude/plugins", workspace))` followed by `.load_installed()`; both are sync, no `block_on` needed.

Do NOT add `Mutex<CheckpointStore>` to `AppCore` this phase (checkpoints are in-memory only with no persistence; surfacing them would mislead users about durability — documented non-goal below).

### 8. New Tauri plugin dependencies and capability entries

Add to `src-tauri/Cargo.toml`:
```
tauri-plugin-dialog  = "2"
tauri-plugin-opener  = "2"
reqwest              = { version = "0.12", features = ["json"] }
```
(`reqwest` must be a direct dependency of `src-tauri`; the transitive presence in `Cargo.lock` via `open_assistant` does not make it usable in `src-tauri`'s own code.)

Register in `src-tauri/src/lib.rs` before `.setup()`:
```rust
.plugin(tauri_plugin_dialog::init())
.plugin(tauri_plugin_opener::init())
```

Update `src-tauri/capabilities/default.json` from `["core:default"]` to:
```json
{
  "identifier": "default",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "dialog:default",
    {
      "identifier": "opener:allow-open-url",
      "allow": [
        { "url": "https://openrouter.ai/*" },
        { "url": "https://openai.com/*" },
        { "url": "https://platform.openai.com/*" }
      ]
    }
  ]
}
```
Use specific domains rather than `https://*` (per Senior Dev: the blanket wildcard is unnecessary and harder to audit). The `dialog:default` permission covers `pick_folder` on all three desktop targets. Do NOT add `core:webview:allow-create-webview-window` — the wizard uses SPA routing in the existing window, not a second window.

### 9. Command module reorganisation

Rename `src-tauri/src/commands.rs` to `src-tauri/src/commands/mod.rs` and extract logical groups into:
- `commands/chat.rs` — `send_message`, `get_history`, `get_status`, `clear_conversation`
- `commands/settings.rs` — `get_config`, `save_config`, `set_tools_enabled`
- `commands/onboarding.rs` — `save_onboarding_config`, `check_path_writable`, `pick_data_dir`, `probe_connection`
- `commands/memory.rs` — `get_memory_md`, `write_memory_md`, `get_today_note`, `append_memory`, `search_memory_files`
- `commands/skills.rs` — `list_skills`, `read_skill`, `create_skill`
- `commands/system.rs` — `run_doctor`

The single `generate_handler![]` in `lib.rs` lists all commands with module-qualified names. Add a comment: `// SINGLE invoke_handler — Tauri discards all but the last registration. Add new commands here.`

### 10. `data-testid` hooks and mock backend extensions

Add `data-testid` attributes for every new surface:
- Wizard: `onboard-overlay`, `onboard-step-bar`, `onboard-workspace-path`, `onboard-provider-radio-openrouter/openai/custom`, `onboard-api-key`, `onboard-test-connection`, `onboard-test-result`, `onboard-tools-card-a`, `onboard-tools-card-b`, `onboard-username`, `onboard-finish-cta`.
- Memory panel: `memory-list`, `memory-content`, `memory-search`, `memory-save`, `memory-append`.
- Skills panel: `skills-list`, `skills-content`, `skills-new`.
- Status panel: `status-card`, `doctor-run-btn`, `doctor-results`.
- Settings: `settings-section-nav`, `settings-user-name`, `settings-data-dir`, `settings-rerun-wizard`.

Extend `window.__MOCK_BACKEND__` in `frontend/app.js` and the Playwright test mock in `tests/e2e/` to stub all new commands with sensible defaults, keeping the existing browser-mock harness pattern.

### 11. Rust tests for new commands

Add `#[cfg(test)]` tests (the first test suite in the repo, following the pattern planned in tasks.md 5.1):
- `save_onboarding_config` round-trip: write all wizard fields via load/mutate/save, reload, assert `data_dir` / `user_name` / `provider` / `model` / `api_base` / `tools_enabled` / `skills.dirs` persisted; assert `api_key` is non-empty but masked in the DTO.
- `run_doctor` returns 6 `DiagnosticResult` entries with `ok: bool` (does not panic on a missing Gemini binary).
- `probe_connection` with a deliberately bad key returns a distinct auth-failure error (not a network-failure or panic).
- Config round-trip for the new `tools.enabled` field: write `true`, reload, assert `true`.

## Impact

**Affected specs:**
- `openspec/changes/add-desktop-app/spec.md` — requirements D4.1 (Settings), D4.2 (API-key gate), D4.4 (tools section), and the Memory/Skills stubs are directly extended by this change.
- New capability: `desktop-onboarding-options` (this change).

**Affected / new code:**
- `src/config/mod.rs` — add `[tools]` table with `enabled: bool` (default `false`); update `Config` struct and `config::save` round-trip.
- `src-tauri/Cargo.toml` — add `tauri-plugin-dialog`, `tauri-plugin-opener`, `reqwest` as direct dependencies.
- `src-tauri/src/lib.rs` — register new plugins; extend `generate_handler!`; read `config.tools.enabled` in `build_core()`.
- `src-tauri/src/state.rs` — add `plugins: Mutex<PluginMarketplace>` to `AppCore`; initialize in `build_core()`.
- `src-tauri/src/commands.rs` → `src-tauri/src/commands/` (module split, no behavior change).
- `src-tauri/src/commands/onboarding.rs` — NEW: `save_onboarding_config`, `check_path_writable`, `pick_data_dir`, `probe_connection`.
- `src-tauri/src/commands/memory.rs` — NEW: `get_memory_md`, `write_memory_md`, `get_today_note`, `append_memory`, `search_memory_files`.
- `src-tauri/src/commands/skills.rs` — NEW: `list_skills`, `read_skill`, `create_skill`.
- `src-tauri/src/commands/system.rs` — NEW: `run_doctor`.
- `src-tauri/src/commands/settings.rs` — extend `ConfigDto` and `save_config` for new fields; update `set_tools_enabled` to persist to config.
- `src-tauri/capabilities/default.json` — add `dialog:default` and scoped `opener:allow-open-url`.
- `frontend/` — onboarding wizard overlay (4 screens), Settings two-column refactor, Memory panel, Skills panel, Status/Doctor panel; nav rail rename "Integrations" → "Status"; `data-testid` hooks; mock backend extensions.
- `tests/e2e/` — Playwright specs for wizard flow and new panels.

**New Tauri commands (summary):**
`probe_connection`, `check_path_writable`, `pick_data_dir`, `save_onboarding_config`, `get_memory_md`, `write_memory_md`, `get_today_note`, `append_memory`, `search_memory_files`, `list_skills`, `read_skill`, `create_skill`, `run_doctor`.

**Extended Tauri commands:**
`get_config` / `get_status` (extended DTOs), `save_config` (new fields), `set_tools_enabled` (now persists to `config.yaml`).

**New Tauri plugins:**
`tauri-plugin-dialog` (folder picker), `tauri-plugin-opener` (external URLs).

## Non-Goals

The following terminal capabilities were evaluated and are explicitly excluded from this change. They MUST NOT appear as working desktop features, active buttons, or implied future items in the UI of this change. Each exclusion is grounded in verified source behavior.

- **`spawn_agent` / sub-agent execution.** `execute_subagent()` in `src/core/subagent.rs:267-283` returns a hardcoded string (`"Sub-agent NAME completed task: GOAL\nSteps: 0/N"`) and does not call the LLM. Agents may be listed read-only (definitions loaded from `.claude/agents/`) with an explicit "Execution not yet available" note, but no Run button.

- **`run_workflow`.** `WorkflowEngine::execute()` in `src/core/workflows.rs` marks every step as `Completed` and returns `"Step X completed: description"` — no LLM is called. Workflows may be listed read-only with step counts, but no Run button.

- **Checkpoint restore / session snapshots.** `CheckpointStore::new()` is in-memory only (doc comment incorrectly claims SQLite persistence; `src/core/checkpoint.rs:31`). Adding `Mutex<CheckpointStore>` to `AppCore` would give within-session snapshots, but surfacing "restore" implies cross-restart durability that does not exist. Deferred until SQLite persistence is implemented.

- **Plugin enable/disable toggle.** `PluginMarketplace::set_enabled()` mutates in-memory state only and does not write back to `plugin.json`. A toggle that silently resets on restart is worse than no toggle. The `Mutex<PluginMarketplace>` is added to `AppCore` state (item 7 above) for future use, but no enable/disable UI is exposed this phase.

- **Marketplace plugin install.** `PluginSource::Marketplace` always returns `Err("Marketplace not yet configured")`. The UI for this path is hidden entirely.

- **Top-level Integrations nav panel.** The label implies OAuth connect flows (Discord bot, Telegram link). Gateway is mostly placeholder. Gateway token *fields* are surfaced in Settings > Channels with an experimental callout, but there is no top-level Integrations panel.

- **Update button.** The CLI `Update` command (`src/main.rs:119`) prints `"Use cargo update && cargo build..."`. No real self-update mechanism exists. Only a version string is shown in Settings > About.

- **Skill activation toggle.** `SkillEngine::activate_skill()` is not checked anywhere in `Agent::process`; enabling a skill has no observable effect on agent behavior.

- **`Danger Zone` destructive memory operations.** Clear-all-memory and reset-config commands do not exist in the current CLI. Implementing them requires new destructive commands and the confirmed-dialog pattern; deferred to a dedicated memory-management change.

- **Multi-agent, agent teams, goal deliberation, plan mode.** All return placeholder text from `src/core/agent.rs`. Not exposed.

- **Streaming chat.** Already deferred in `add-desktop-app` as a specified P1 follow-up using `tauri::ipc::Channel<StreamEvent>`. Not in scope here.

- **Calling `wizard.rs::run_wizard()` from the desktop.** That function uses `std::io::stdin().read_line()` and `println!()` — it blocks and cannot run in a Tauri WebView. The desktop wizard is entirely frontend-driven through Tauri commands and shares no code path with the CLI wizard.
