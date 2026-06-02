# Design: add-desktop-onboarding-options

## Context

The `add-desktop-app` change shipped a working Tauri 2.x desktop shell with:
- A real, wired `send_message` command calling `Agent::process` (not a stub).
- A nav rail (Chat active; Memory/Integrations/Skills disabled as "soon"; Settings).
- A Settings view exposing `model.model`, `model.api_base`, masked `model.api_key`, and a tools toggle.
- A first-run API-key gate that routes to Settings when `config.model.api_key` is empty.
- Commands: `send_message`, `get_history`, `get_status`, `clear_conversation`, `get_config`, `save_config`, `set_tools_enabled`.
- Capability permissions: `core:default` only.
- CSS tokens from `src/ui/web.rs` `:root` (Inter, `#2563eb`, `#f59e0b`, `#f8fafc`, `#e2e8f0`).
- A Playwright E2E harness with `window.__MOCK_BACKEND__` / `defaultMock`, a `static-server.cjs` dev server, and stable `data-testid` hooks on all interactive elements.

This change adds a proper onboarding wizard and surfaces the terminal's real, non-stubbed capabilities as desktop panels. The expert panel's unanimous verdict: **scope is the killer**. Three things survive:

1. **Great onboarding.** The CLI has a 6-step stdin wizard (`src/onboarding/wizard.rs`) that blocks on `io::stdin()` and cannot be called from Tauri. The desktop needs a 4-screen SPA overlay that calls `config::save()` directly, with a real connection test that never touches `Agent::process`.
2. **Real panels.** Memory browser, Skills manager, and Status/Doctor have verified-real core implementations. They become real nav destinations.
3. **Expanded Settings.** The flat 4-field form becomes a two-column sectioned panel covering all non-stub `Config` fields.

Every terminal feature that is a confirmed stub (sub-agent execution, workflow execution, checkpoint restore, marketplace plugin install, self-update) is explicitly named with source-line citations and NOT surfaced in this change.

## Goals / Non-Goals

### Goals

1. **4-screen onboarding wizard** — a full-screen, non-dismissible SPA overlay shown on first run (api_key empty or no config): Workspace picker, AI Provider + inline connection test, Tools and Permissions consent, Identity + Finish. Re-enterable from three entry points.
2. **`probe_connection` command** — raw `reqwest` POST with `max_tokens=1`, bypassing `Agent::process` entirely. Four distinct outcomes: success, auth failure (401/403), model unavailable (404), network failure.
3. **`tools.enabled` persistence** — add a `[tools] enabled: bool` field to `src/config/mod.rs` `Config` struct so the consent choice from Screen 3 survives app restart. The existing `set_tools_enabled` command writes only to in-memory state; this is the gap the wizard depends on.
4. **`get_app_state` command** — returns `initial_view: String` (`"onboarding"` | `"banner"` | `"chat"`) and `onboarding_complete: bool` so the frontend can route before first paint without a chat-view flash.
5. **`save_onboarding_config` command** — accepts a flat `OnboardingDto` covering all wizard fields and writes them atomically via `config::load()` → mutate → `config::save()`. Never routes through `config::set()`.
6. **`tauri-plugin-dialog` folder picker** — wraps `blocking_pick_folder()` in `spawn_blocking`, converts via `FilePath.as_path().display().to_string()` (not `.to_string()` on the enum), and displays OS-native path separators on Windows.
7. **`tauri-plugin-opener`** for external links (provider dashboard URLs), with `opener:allow-open-url` scoped to specific domains, not a wildcard.
8. **Expanded Settings view** — two-column layout (category sidebar + content panel) with 7 sections: Model, General, Tools, Memory, Skills, Channels, Advanced. Gateway token fields appear in Channels with an "experimental" callout. Version shown in a footer; no Update button.
9. **Memory browser** — Memory nav item wired; shows MEMORY.md (editable textarea), today's daily note (read-only), and a search box backed by `MemoryWorkspace::search_files`. Four new Tauri commands: `get_memory_md`, `write_memory_md`, `get_today_note`, `search_memory_files`.
10. **Skills manager** — Skills nav item wired; lists built-ins (via `SkillEngine::load_builtin()`) and user skills (via `load_from_dir`); read and create. No activation toggle (not wired into `Agent::process`). Three new commands: `list_skills`, `read_skill`, `create_skill`.
11. **Status/Doctor panel** — "Integrations" nav item renamed to "Status"; shows extended status + a Doctor diagnostics panel (six checks, colored pass/fail rows, run-on-demand). Extended `get_status` gains `memory_db_entries` and `memory_md_chars`. New `run_doctor` command returns `Vec<DiagnosticResult>`. Agents listed read-only from definitions; no Run button.
12. **Command module migration** — `src-tauri/src/commands.rs` promoted to `src-tauri/src/commands/` directory with sub-modules (`chat.rs`, `settings.rs`, `onboarding.rs`, `memory.rs`, `skills.rs`, `system.rs`). Single `generate_handler!` invariant preserved with a guard comment.
13. **All existing patterns reused** — single `tokio::sync::Mutex` turn state, masked key in all DTOs, tools-off default, locked CSP unchanged, `data-testid` + `__MOCK_BACKEND__` for Playwright, full `load/mutate/save` config writes everywhere.

### Non-Goals (explicitly NOT in this change)

The following are confirmed stubs in source. They are named with source-line citations to prevent a future contributor re-adding "Run" buttons.

| Feature | Stub evidence | Desktop policy |
|---|---|---|
| Sub-agent execution | `src/core/subagent.rs:267-283` — `execute_subagent()` returns a hardcoded `"Sub-agent NAME completed task: GOAL\nSteps: 0/N"` string regardless of input; no LLM call | NOT surfaced. Agents view is read-only definitions list only. No `spawn_agent` command. |
| Workflow execution | `src/core/workflows.rs:141-161` — each step closure formats `"Step X completed: description"`; no LLM call; `list_runs()` always returns empty `Vec` | NOT surfaced. No `run_workflow` command. |
| Checkpoint restore | `src/core/checkpoint.rs:31` — `CheckpointStore::new()` creates an in-memory `Vec`; no SQLite persistence despite the file comment claiming it | NOT surfaced this phase. |
| Plugin marketplace install | `src/core/plugins.rs:216` — `PluginSource::Marketplace` always returns `Err("Marketplace not yet configured")` | NOT surfaced. Marketplace button hidden. Local-path install deferred to a future change. |
| Plugin enable/disable persistence | `src/core/plugins.rs` — `set_enabled()` mutates in-memory state only; no write-back to `plugin.json` | Plugin toggle not exposed until persistence is implemented. |
| Self-update | `src/main.rs:119` — `Update` command prints `"Use cargo update && cargo build..."` | NOT surfaced. Version-only footer in Settings. No Update button. |
| Gateway live dashboard | `src/gateway/` — Discord/Telegram/Slack are placeholder implementations | NOT surfaced. Gateway tokens appear in Settings > Channels for config only, with an "experimental" callout. No connection-status display. |
| Skill activation toggle | `src/skills/engine.rs` — `activate_skill()` is implemented but the allowed/disallowed tool restrictions are never read in `Agent::process` | NOT exposed. Skills view shows list/read/create only. |
| `config::set()` path | `src/config/mod.rs:158-172` — `_` arm silently `warn!`s and discards unknown keys | Never used by any desktop command. All writes use `load/mutate/save`. |
| LLM token streaming | `Agent::process` returns one final string; streaming is P1 via `tauri::ipc::Channel<StreamEvent>` (spec'd in `add-desktop-app` D6) | Not in this change. |
| Dark theme, tray, global hotkey, history persistence, animated mascot | Per `add-desktop-app` Non-Goals | Not in this change. |

## Decisions

### D1 — Onboarding wizard: in-app SPA overlay, not a second window

**Decision:** The wizard is a full-screen overlay rendered inside the existing `main` window, driven by a SPA routing state variable (`currentView = "onboarding"`), not a second `WebviewWindowBuilder` window.

**Rationale:** A second window requires `core:webview:allow-create-webview-window` in capabilities, a second window label, careful event wiring to signal completion back to the main window, and a `WebviewWindowBuilder::build()` in an async command (synchronous window creation deadlocks on Windows due to WebView2's message-pump requirement). The first-run wizard is not a separate product — it configures the app and then gives way to Chat. SPA routing in the existing window is the idiomatic Tauri 2 pattern for this and eliminates all the above complexity.

**Overlay behavior:** The nav rail is visible behind the overlay but `aria-disabled` and non-interactive. The wizard card sits centered at `max-width: 620px`, `background: #fff`, `border: 1px solid #e2e8f0`, `border-radius: 12px`, box-shadow card style matching the existing aesthetic. The overlay backdrop is `rgba(15,23,42,0.5)`. Dismissal by clicking outside is disabled; the only exit paths are completing the wizard or (on re-entry) clicking "Close" on the Finish screen.

**First-run routing logic:** A new `get_app_state` command returns `initial_view: "onboarding" | "banner" | "chat"` computed as follows:
- `initial_view = "onboarding"` when `config.model.api_key.is_empty()` — covers fresh installs and the existing API-key gate.
- `initial_view = "banner"` when `config.general.user_name` is empty or `"User"` (the default) while api_key is set — shows a dismissible top-banner "What should I call you?" after the first message, maps to wizard step 4.5.
- `initial_view = "chat"` otherwise — the happy path; direct to Chat, no overlay.

The frontend calls `get_app_state` in its initialization block before rendering any view, using the returned `initial_view` to set the initial route. This replaces the existing `get_config` api_key check in `app.js` that currently routes to Settings — it now routes to the wizard overlay instead.

**Re-entry:** Three entry points must exist: (a) Settings view → "Re-run Setup Wizard" button below the Save button; (b) Status/Doctor panel → "Run Setup Wizard" link when api_key is empty or a diagnostic check fails; (c) the launch gate itself (via `get_app_state`) if api_key becomes empty after a config wipe.

### D2 — Wizard screens: 4 screens mapping the CLI's 6 steps

**Decision:** The wizard has 4 screens. The CLI's 6 steps in `src/onboarding/wizard.rs` are mapped as follows:

| CLI step | Field(s) | Desktop screen | Note |
|---|---|---|---|
| 1 — workspace/data_dir | `general.data_dir` | Screen 1 — Workspace | Pre-filled with OS default; native folder picker |
| 2 — provider/model/api_key/api_base | `model.*` | Screen 2 — AI Provider | Inline connection test before Continue unlocks |
| 3 — Discord/Telegram tokens | `gateway.discord_token`, `gateway.telegram_token` | DEFERRED to Settings > Channels | Gateway is placeholder; these are not a first-run blocker |
| 4 — dm_pairing security | `security.dm_pairing`, `gateway.dm_policy` | DEFERRED to Settings > Channels | Same rationale |
| 4.5 — user_name | `general.user_name` | Screen 4 — Identity + Finish | Optional; default "friend" |
| 5 — vision/gemini check | calls `tools::vision::check()` | Async pill on Screen 4 | Non-blocking; 200ms timeout |
| 6 — skills dirs | `skills.dirs` | Auto-defaulted to `~/.openassistant/skills` | Not surfaced in wizard; advanced users use Settings |

**Step indicator:** A horizontal progress bar at the top of the wizard card shows: `1 Workspace · 2 AI Provider · 3 Permissions · 4 Finish`. Filled circles (`#2563eb`) for completed steps; animated ring for the current step; grey for future steps. Labels are shown so users know how many screens remain.

**Screen 1 — Workspace (`data-testid="onboard-workspace"`):**
- Title: "Where should I keep your data?"
- Body: styled path chip showing the OS default (`%USERPROFILE%\.openassistant` on Windows; `$HOME/.openassistant` on other platforms) with OS-native separators.
- "Change folder" button opens the native folder picker via `tauri-plugin-dialog`.
- Async writability badge (green "Writable" / red "Cannot write here — choose another") backed by `check_path_writable` command.
- Continue disabled while writability check is pending.
- On advance: creates the dir and `skills/`, `memory/` subdirs before transitioning.

**Screen 2 — AI Provider (`data-testid="onboard-provider"`):**
- Three provider radio cards: OpenRouter (recommended, pre-fills `api_base`), OpenAI (pre-fills `api_base`), Custom (reveals editable `api_base` field).
- Model text field; pre-filled per provider (`openrouter/auto` for OpenRouter, `gpt-4o` for OpenAI, empty for Custom).
- API key: password input with show/hide toggle. Placeholder "sk-..." or "your-key-here". Helper link "Get an OpenRouter key →" opens the provider URL via `tauri-plugin-opener` (NOT in the webview; CSP stays intact).
- "Test connection" primary button: fires `probe_connection`; shows spinner while pending; on success: green banner "Connected! ({latency_ms}ms)"; Continue unlocks. On failure: red inline error with the specific failure type.
- Continue stays locked until a passing test. Editing api_key, api_base, or model clears the cached test result and re-locks Continue.

**Screen 3 — Tools and Permissions Consent (`data-testid="onboard-permissions"`):**
- Non-skippable. Title: "How much access should openAssistant have?"
- Card A (default, pre-selected): lock icon, "Chat only — no file or shell access. Recommended." (`data-testid="onboard-tools-card-a"`)
- Card B: terminal icon, "Enable tools — the assistant can run shell commands and read/write files." (`data-testid="onboard-tools-card-b"`)
- If Card B selected: amber banner "You can change this any time in Settings."
- Continue enabled only after one card is clicked. Writes `tools.enabled` to disk via `save_onboarding_config`.

**Screen 4 — Identity + Finish (`data-testid="onboard-finish"`):**
- Title: "One last thing — what should I call you?"
- Text input, placeholder "friend", max 64 chars, optional. Skip link at top-right.
- Three async status pills that update independently: "Config saved", "Data directory ready", "Vision tools" (green "Gemini CLI detected" / amber "Not found — image analysis unavailable" with 200ms timeout).
- Large "Start chatting →" CTA (`data-testid="onboard-finish-cta"`).
- Back button hidden; a small "Back" text link is acceptable.
- Footer hint: "You can re-run this setup from Settings > Re-run Setup Wizard at any time."

### D3 — `probe_connection`: raw reqwest, bypassing `Agent::process`

**Decision:** `probe_connection` calls `reqwest::Client::new().post(...)` directly, NOT through `Agent::process` or any agent path.

**Rationale (Senior Dev, confirmed):** `Agent::process` has persistent side effects: it calls `mem.append_daily()` (writes to `memory/YYYY-MM-DD.md`) and `ctx.observe()` (mutates `UserModel`) on every invocation. Running the agent on the very first interaction with the app — the connection test — would write daily-note entries and corrupt the user model before the user has sent a single real message. The test must be a pure network probe.

**Implementation (`src-tauri/src/commands/system.rs`):**
```rust
#[tauri::command]
pub async fn probe_connection(
    api_key: String,
    api_base: String,
    model: String,
) -> Result<ProbeResult, String> {
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1
    });
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await;
    match resp {
        Ok(r) => match r.status().as_u16() {
            200..=299 => Ok(ProbeResult { ok: true, error_type: None,
                               latency_ms: 0, message: "Connected".into() }),
            401 | 403 => Ok(ProbeResult { ok: false,
                               error_type: Some("auth_failure".into()),
                               latency_ms: 0,
                               message: "Invalid API key".into() }),
            404       => Ok(ProbeResult { ok: false,
                               error_type: Some("model_unavailable".into()),
                               latency_ms: 0,
                               message: "Model not found — try a different model name".into() }),
            s         => Ok(ProbeResult { ok: false,
                               error_type: Some("api_error".into()),
                               latency_ms: 0,
                               message: format!("API returned status {s}") }),
        },
        Err(e) if e.is_timeout() || e.is_connect() =>
            Ok(ProbeResult { ok: false, error_type: Some("network_failure".into()),
                             latency_ms: 0,
                             message: "Could not reach the endpoint — check the URL and your network".into() }),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(serde::Serialize)]
pub struct ProbeResult {
    pub ok: bool,
    pub latency_ms: u32,
    pub error_type: Option<String>,
    pub message: String,
}
```

**Four distinct outcomes — not three:** Success, auth failure (401/403), model unavailable (404), network failure. A 404 on the default `openrouter/owl-alpha` model (which may be unavailable) must not show "Invalid API key" — it shows "Model not found"; a user with a valid key can change the model and retry. Collapsing 404 into auth failure would block valid users.

**Dependency:** Add `reqwest = { version = "0.12", features = ["json"] }` as a **direct** dependency in `src-tauri/Cargo.toml`. A transitive dependency in `Cargo.lock` is not usable from `src-tauri`'s own `use reqwest::...`; it must be listed explicitly.

### D4 — `tools.enabled` persistence: add `[tools]` section to `Config`

**Decision:** Add a `tools` section to `src/config/mod.rs` so the consent choice from Screen 3 is written to `config.yaml` and survives restart.

**The gap:** The existing `set_tools_enabled` command (`src-tauri/src/commands.rs`) writes only to `AppCore` managed state. The `Config` struct has no `tools` field. The wizard's Permissions screen is meaningless if the user's consent resets to "off" every time the app closes.

**Change to `src/config/mod.rs`:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    pub enabled: bool,
}

pub struct Config {
    pub general:  GeneralConfig,
    pub model:    ModelConfig,
    pub gateway:  GatewayConfig,
    pub memory:   MemoryConfig,
    pub skills:   SkillsConfig,
    pub security: SecurityConfig,
    pub vision:   VisionConfig,
    pub tools:    ToolsConfig,   // NEW
}
```

The `Default` impl gives `tools.enabled = false`, so existing `config.yaml` files without a `[tools]` section deserialize correctly (serde_yaml applies the field default when the key is absent). No migration needed.

**`build_core()` update (`src-tauri/src/lib.rs`):** Read `cfg.tools.enabled` when constructing `AppCore` so the persisted consent takes effect on startup:
```rust
let tools_enabled = cfg.tools.enabled;
// ... build agent, turn state ...
AppCore { agent, turn: tokio::sync::Mutex::new(Turn { ctx, session, tools_enabled }), ... }
```

**`save_onboarding_config`** writes `cfg.tools.enabled = dto.tools_enabled` and then calls `config::save(&cfg)` so the choice is durable.

### D5 — Folder picker: `tauri-plugin-dialog`, threading, and FilePath conversion

**Decision:** Use `tauri-plugin-dialog` for the native OS folder picker on Screen 1. Wrap `blocking_pick_folder()` in `tauri::async_runtime::spawn_blocking`. Convert the result via `FilePath.as_path().display().to_string()`.

**Three correct patterns for the async command context; choose spawn_blocking:**
- `blocking_pick_folder()` in an `async #[tauri::command]` (Tauri runs these on a Tokio worker thread, not the main UI thread) — works but starves the Tokio pool on a slow dialog.
- `spawn_blocking` — explicitly moves the blocking call off the async executor.
- Non-blocking `pick_folder(|path| ...)` callback bridged via `tokio::sync::oneshot` — also correct, more complex.

`spawn_blocking` is the idiomatic choice. **Do NOT call `blocking_pick_folder()` on the main thread** — that deadlocks the event loop.

**FilePath pitfall (Senior Dev):** The return type is `tauri_plugin_dialog::FilePath`, which is an enum wrapping a `PathBuf` on desktop and a URI string on mobile. Calling `.to_string()` on the enum serializes the variant name, not the path. Use `.as_path().map(|p| p.display().to_string())` on desktop.

**Windows path display:** The CLI wizard uses `$HOME/$USERPROFILE + /.openassistant`, which on Windows becomes `C:\Users\Username\.openassistant`. Use `std::path::Path::display()` which respects the OS separator — on Windows this renders backslashes. Do NOT hardcode forward slashes in the path chip.

**Capabilities addition** (`src-tauri/capabilities/default.json`): add `"dialog:default"` to the permissions array. This grants `dialog:allow-open`, `dialog:allow-save`, and `dialog:allow-message` automatically.

```rust
#[tauri::command]
pub async fn pick_data_dir(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|fp| fp.as_path().map(|p| p.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())
}
```

**`check_path_writable` command:**
```rust
#[tauri::command]
pub async fn check_path_writable(path: String) -> Result<bool, String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        tokio::fs::create_dir_all(p).await.map_err(|e| e.to_string())?;
    }
    let test_file = p.join(".openassistant_write_test");
    match tokio::fs::write(&test_file, b"").await {
        Ok(_) => { let _ = tokio::fs::remove_file(&test_file).await; Ok(true) }
        Err(_) => Ok(false),
    }
}
```

### D6 — Nav rail and IA: "Integrations" → "Status"; Settings becomes two-column

**Decision:** The nav rail's "Integrations" item is renamed to "Status" and wired to the Status/Doctor panel. Settings gains a two-column sectioned layout.

**Rationale for renaming Integrations:** "Integrations" in a desktop assistant implies OAuth connect flows (Discord bot connect, Telegram link, Slack OAuth). The gateway module is primarily placeholder code (`src/gateway/discord.rs`, `src/gateway/telegram.rs`). Advertising an "Integrations" nav item — even disabled — invites user questions and reviewer flags about when it will be clickable. "Status" is accurate (it shows real diagnostics), honest (it does not imply OAuth), and useful (users benefit from a health check panel).

**Final nav rail order:**
1. Chat (active, wired)
2. Memory (wired in this change, P1 was pre-approved)
3. Skills (wired in this change)
4. Status (formerly Integrations; wired in this change)
5. Settings (wired, expanded in this change)

**Settings two-column layout:** The current flat 4-field card does not scale to 7 sections. The content area of the Settings view gains a secondary left column (128px, category list) and a right content column. Category list items: Model, General, Tools, Memory, Skills, Channels, Advanced. This matches the Raycast / VS Code / Zed settings pattern. The nav rail remains visible at all times; the Settings sub-nav is within the content area only.

**Settings sections:**

| Section | Fields |
|---|---|
| Model | `model.provider` (radio), `model.model`, `model.api_base`, `model.api_key` (masked) |
| General | `general.user_name`, `general.data_dir` (read-only path chip + "Open folder" button), `general.log_level` (dropdown) |
| Tools | `tools.enabled` toggle + existing warning copy |
| Memory | `memory.max_entries`, `memory.fts_enabled` toggle, `memory.db_path` (read-only) |
| Skills | `skills.dirs` list, `skills.auto_create` toggle |
| Channels | `gateway.discord_token`, `gateway.telegram_token`, `gateway.slack_token` (all masked, same treatment as api_key) with callout: "Gateway channel integration is experimental — the messaging server is not yet fully operational." |
| Advanced | `vision.provider`, `vision.gemini_path` |

**Version footer (not an Update button):** A `version` string returned from `get_app_state` (or a static `env!("CARGO_PKG_VERSION")` in a helper command) appears in the Settings footer: "openAssistant vX.Y.Z". No "Check for updates" button — the CLI Update command (`src/main.rs:119`) prints `cargo update && cargo build` instructions and is not a real self-update mechanism.

**Re-run wizard entry point:** Settings footer also contains a "Re-run Setup Wizard" text link below the Save button in every section. Style: secondary/text, `data-testid="settings-rerun-wizard"`.

### D7 — Memory browser: `MemoryWorkspace` commands

**Decision:** Wire the Memory nav item to a two-pane view backed by four new Tauri commands calling `MemoryWorkspace` methods from `src/core/memory.rs`. All six `MemoryWorkspace` methods (`read_long_term`, `write_long_term`, `append_long_term`, `read_today`, `append_daily`, `search_files`) are verified real file I/O.

**View layout:**
- Left pane: file list showing MEMORY.md, today's daily note (`memory/YYYY-MM-DD.md`), and search results. A search box at the top filters via `search_memory_files`.
- Right pane: content area — MEMORY.md is shown in an editable textarea (saved via `write_memory_md`); daily notes are shown read-only; search results are shown as read-only excerpts.

**New commands (`src-tauri/src/commands/memory.rs`):**
```
get_memory_md()                   -> Result<String, String>
write_memory_md(content: String)  -> Result<(), String>
get_today_note()                  -> Result<String, String>
search_memory_files(query: String)-> Result<Vec<[String; 2]>, String>
```

Each constructs `MemoryWorkspace::from_data_dir(&cfg.general.data_dir)` by calling `config::load().await` at the start of the command (the existing pattern from `get_status`).

**`MemoryStore` (SQLite/FTS5):** Not surfaced in the Memory browser in this change. The SQLite store is opened per-call (`MemoryStore::open_default().await`), which is safe but slightly expensive (~5ms per call). Adding a `Mutex<Option<MemoryStore>>` to `AppCore` is a P1 optimization. For this change, `memory_db_entries` is read in `get_status` via a one-shot open+count+close.

### D8 — Skills manager: `SkillEngine`, no activation

**Decision:** Wire the Skills nav item to a two-pane view backed by three new Tauri commands. Skill activation is NOT exposed — `activate_skill()` exists in `SkillEngine` but the allowed/disallowed tool restrictions are never read in `Agent::process`.

**View layout:**
- Left pane: list of skills from `SkillEngine::load_builtin()` (3 built-ins) + `load_from_dir(config.skills.dirs[0])` (user skills). Each item shows name, category, and a "built-in" badge where applicable.
- Right pane: content (markdown) for the selected skill. A "New Skill" button opens a modal with name + content fields.

**New commands (`src-tauri/src/commands/skills.rs`):**
```
list_skills()                               -> Result<Vec<SkillDto>, String>
read_skill(name: String)                    -> Result<String, String>
create_skill(name: String, content: String) -> Result<(), String>
```

`list_skills` calls `SkillEngine::load_builtin()?.list()` for built-ins, then if `config.skills.dirs` is non-empty, calls `load_from_dir` on the first dir and appends. Returns `Vec<SkillDto { name, description, category, is_builtin }>.`

`create_skill` writes via `tokio::fs::write(format!("{}/{}.md", dir, name), content)`. The dir is `config.skills.dirs[0]` if present, else creates `{data_dir}/skills/` and writes there.

**Skills count in Doctor:** The CLI's `skills::check()` calls `SkillEngine::default().count()` which returns 0 because `default()` does not call `load_builtin()`. The `run_doctor` command MUST use `SkillEngine::load_builtin()?.count()` (returns 3) to show an accurate count. This is a known bug in the CLI diagnostics; the desktop should not replicate it.

### D9 — Status/Doctor panel: `run_doctor`, extended `get_status`, read-only agents

**Decision:** The Status panel has three cards: Live Status (from extended `get_status`), Doctor Diagnostics (from new `run_doctor`), and Agent Definitions (read-only list from `SubAgentOrchestrator::load_definitions`).

**Extended `get_status`:** Add `memory_db_entries: i64` and `memory_md_chars: usize` to `StatusResponse`:
```rust
// In src-tauri/src/commands/chat.rs (or system.rs)
let store = open_assistant::memory::store::MemoryStore::open_default().await
    .map_err(|e| e.to_string())?;
let memory_db_entries = store.count().await.unwrap_or(0);
let ws = open_assistant::core::memory::MemoryWorkspace::from_data_dir(&cfg.general.data_dir);
let memory_md_chars = ws.read_long_term().len();
```

**`run_doctor` command (`src-tauri/src/commands/system.rs`):** Runs all six diagnostic checks from `src/main.rs:304-343`, captures each result, returns `Vec<DiagnosticResult>`:
```rust
#[derive(serde::Serialize)]
pub struct DiagnosticResult {
    pub name: String,
    pub ok: bool,
    pub message: String,
    pub is_warning: bool,  // amber vs red — for non-critical missing tools
}
```

Six checks:
1. `config::check()` — calls `config::load()`. Pass/fail.
2. `MemoryStore::open_default().await` — SQLite open + schema init. Pass/fail.
3. `MemoryWorkspace::from_data_dir(&data_dir).init().await` — creates dirs + MEMORY.md. Pass/fail.
4. `SkillEngine::load_builtin()?.count()` — returns 3 (not `default().count()` which returns 0). Pass/fail.
5. `gateway::check()` — checks gateway config; returns Err unless tokens set. If Err: `is_warning = true` (amber), message "No gateway tokens configured — messaging channels inactive". Not a red error.
6. `tools::vision::check()` with a **200ms timeout** — spawns `gemini --skip-trust whoami`. If timeout or binary not found: `is_warning = true` (amber), message "Gemini CLI not found — image analysis unavailable". Not a red error (Gemini is optional, and `gemini.exe` is commonly absent on Windows clean installs).

**Vision check timeout:**
```rust
let vision_result = tokio::time::timeout(
    std::time::Duration::from_millis(200),
    open_assistant::tools::vision::check()
).await;
match vision_result {
    Ok(Ok(_))  => DiagnosticResult { name: "Vision tools".into(), ok: true,
                                      is_warning: false, message: "Gemini CLI detected".into() },
    Ok(Err(e)) => DiagnosticResult { name: "Vision tools".into(), ok: false,
                                      is_warning: true,
                                      message: format!("Gemini CLI not available: {}", e) },
    Err(_)     => DiagnosticResult { name: "Vision tools".into(), ok: false,
                                      is_warning: true,
                                      message: "Gemini CLI check timed out — binary not on PATH".into() },
}
```

**Read-only agent definitions:** `SubAgentOrchestrator::load_definitions(&format!("{}/.claude/agents", data_dir))` + `list_definitions()` provides real `SubAgentDef` structs (name, description, tools, model). New command `list_agents()` returns `Vec<AgentDto>`. The Status panel shows these in a card with a banner: "Agent execution is not available in the desktop app. Manage agent definitions in `.claude/agents/`." No Run button. No `spawn_agent` command.

**Doctor re-entry link:** If `api_key_set == false` in the Status panel, show a "Run Setup Wizard" action link (`data-testid="status-run-wizard"`) that routes to the wizard overlay.

### D10 — `save_onboarding_config`: one atomic write for all wizard fields

**Decision:** A single `save_onboarding_config(OnboardingDto) -> Result<(), String>` command handles all four wizard screens in one atomic `config::save` call at the end of the wizard. Mid-wizard writes are not performed.

**`OnboardingDto` (Rust, `src-tauri/src/commands/onboarding.rs`):**
```rust
#[derive(serde::Deserialize)]
pub struct OnboardingDto {
    pub data_dir: String,
    pub provider: String,
    pub model: String,
    pub api_base: String,
    pub api_key: String,
    pub tools_enabled: bool,
    pub user_name: Option<String>,
}
```

**Command implementation:**
```rust
#[tauri::command]
pub async fn save_onboarding_config(dto: OnboardingDto) -> Result<(), String> {
    let mut cfg = open_assistant::config::load().await.map_err(|e| e.to_string())?;
    cfg.general.data_dir   = dto.data_dir;
    cfg.general.user_name  = dto.user_name.unwrap_or_else(|| "friend".into())
                                 .trim().chars().take(64).collect();
    cfg.model.provider     = dto.provider;
    cfg.model.model        = dto.model;
    cfg.model.api_base     = dto.api_base;
    if !dto.api_key.is_empty() { cfg.model.api_key = dto.api_key; }
    cfg.tools.enabled      = dto.tools_enabled;
    // skills.dirs: ensure the default dir exists; do not overwrite user customization
    if cfg.skills.dirs.is_empty() {
        cfg.skills.dirs = vec![format!("{}/skills", cfg.general.data_dir)];
    }
    open_assistant::config::save(&cfg).await.map_err(|e| e.to_string())
}
```

**`skills.dirs` handling:** The frontend sends `data_dir`; the command derives the default skills dir server-side (`{data_dir}/skills`). Do NOT expose `skills.dirs` as a comma-separated string in the DTO — `serde_yaml` serializes `Vec<String>` as a YAML sequence; if a comma-separated string were stored, it would appear as a literal comma-separated string in YAML, not a list.

**Key masking:** The wizard pre-fills `api_key` as `"••••...••••"` when re-running on an existing config. The frontend sends an empty string when the user did not change the key; the command only overwrites `api_key` when the incoming value is non-empty. This is consistent with the existing `save_config` pattern in `commands.rs`.

**After save:** `save_onboarding_config` does not rebuild the `Agent` in `AppCore` state. The next message send will call `config::load()` (per D11 in `add-desktop-app`, `call_llm` reloads config every turn), so the new settings take effect immediately on the next interaction without a restart.

### D11 — Command module structure

**Decision:** Promote `src-tauri/src/commands.rs` to a `commands/` directory with sub-modules. The single `generate_handler![]` invariant is preserved.

**Directory structure:**
```
src-tauri/src/
├── commands/
│   ├── mod.rs         -- re-exports; pub use chat::*; pub use settings::*; etc.
│   ├── chat.rs        -- send_message, get_history, get_status, clear_conversation
│   ├── settings.rs    -- get_config, save_config, set_tools_enabled
│   ├── onboarding.rs  -- save_onboarding_config, get_app_state
│   ├── memory.rs      -- get_memory_md, write_memory_md, get_today_note, search_memory_files
│   ├── skills.rs      -- list_skills, read_skill, create_skill
│   └── system.rs      -- probe_connection, check_path_writable, pick_data_dir, run_doctor,
│                          list_agents, list_plugins
├── state.rs
└── lib.rs
```

**`lib.rs` `invoke_handler`:**
```rust
// SINGLE invoke_handler — Tauri keeps only the LAST registration.
// Add new commands here; never add a second .invoke_handler() call.
.invoke_handler(tauri::generate_handler![
    commands::chat::send_message,
    commands::chat::get_history,
    commands::chat::get_status,
    commands::chat::clear_conversation,
    commands::settings::get_config,
    commands::settings::save_config,
    commands::settings::set_tools_enabled,
    commands::onboarding::get_app_state,
    commands::onboarding::save_onboarding_config,
    commands::memory::get_memory_md,
    commands::memory::write_memory_md,
    commands::memory::get_today_note,
    commands::memory::search_memory_files,
    commands::skills::list_skills,
    commands::skills::read_skill,
    commands::skills::create_skill,
    commands::system::probe_connection,
    commands::system::check_path_writable,
    commands::system::pick_data_dir,
    commands::system::run_doctor,
    commands::system::list_agents,
])
```

**`AppCore` additions (`src-tauri/src/state.rs`):** Add `Mutex<PluginMarketplace>` if/when the Plugins panel lands (deferred to a future change). This change does NOT add `PluginMarketplace` to state because the toggle persistence gap means the UI would be misleading. `CheckpointStore` is also not added — checkpoint restore is deferred per the Non-Goals table.

### D12 — `tauri-plugin-opener` capability scoping

**Decision:** Use specific domain allow-list entries, not a wildcard `https://*`.

**Rationale:** A blanket `https://*` allows opening any HTTPS URL if the webview is ever compromised. The wizard only needs to open provider dashboard URLs — three domains are sufficient for v1. The `opener:allow-open-url` capability entry uses explicit origin patterns:

```json
{
  "identifier": "opener:allow-open-url",
  "allow": [
    { "url": "https://openrouter.ai/*" },
    { "url": "https://openai.com/*" },
    { "url": "https://platform.openai.com/*" }
  ]
}
```

**Updated `src-tauri/capabilities/default.json`:**
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
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

**CSP (`tauri.conf.json`):** Unchanged from v1. The opener plugin launches URLs via the OS shell, not via webview fetch, so no CSP change is needed.

**Plugin registration in `src-tauri/src/lib.rs`** (before `.setup()`):
```rust
tauri::Builder::default()
    .plugin(tauri_plugin_log::Builder::default().build())
    .plugin(tauri_plugin_dialog::init())    // NEW
    .plugin(tauri_plugin_opener::init())    // NEW
    .setup(|app| { ... })
    .invoke_handler(...)
```

### D13 — Testing: Rust unit tests, Playwright extensions, `__MOCK_BACKEND__`

**Decision:** All new commands have `#[cfg(test)]` round-trip tests. The Playwright `defaultMock` and `__MOCK_BACKEND__` are extended for every new command. New `data-testid` hooks are added to every new wizard screen and panel.

**New Rust unit tests (`src-tauri/src/commands/tests/`):**
1. `save_onboarding_config` round-trip — writes all DTO fields via the command, reads back via `config::load()`, asserts `data_dir`, `user_name`, `provider`, `model`, `api_base`, `tools.enabled`, `skills.dirs[0]` all persisted correctly.
2. `probe_connection` error mapping — four mock HTTP responses (200, 401, 404, timeout) → four distinct `ProbeResult` values.
3. `mask_key` — existing test from `settings.rs`; extended to cover the `OnboardingDto` api_key passthrough.
4. Extended `get_status` — asserts `memory_db_entries` and `memory_md_chars` fields are present in the response.
5. `run_doctor` structure — asserts six `DiagnosticResult` entries are returned and that the skills check reports `ok: true` with count ≥ 3 (via `load_builtin`, not `default()`).
6. `list_skills` — asserts at least 3 built-in skills returned.

**Playwright `defaultMock` additions (`frontend/app.js`):** Every new command needs a mock entry:
```javascript
const defaultMock = {
  // ... existing entries ...
  get_app_state: async () => ({
    initial_view: "chat", onboarding_complete: true, version: "0.1.0"
  }),
  save_onboarding_config: async () => null,
  probe_connection: async ({ api_key }) =>
    api_key === "valid"
      ? { ok: true, latency_ms: 42, error_type: null, message: "Connected" }
      : { ok: false, latency_ms: 0, error_type: "auth_failure", message: "Invalid API key" },
  check_path_writable: async () => true,
  pick_data_dir: async () => "C:\\Users\\test\\.openassistant",
  get_memory_md: async () => "# Memory\n\n- Example entry",
  write_memory_md: async () => null,
  get_today_note: async () => "# 2026-06-03\n\n- Test note",
  search_memory_files: async () => [["2026-06-03", "found content"]],
  list_skills: async () => [
    { name: "coder", description: "Coding assistant", category: "development", is_builtin: true },
  ],
  read_skill: async () => "---\ntitle: coder\n---\nHelp with code.",
  create_skill: async () => null,
  run_doctor: async () => [
    { name: "Config", ok: true, is_warning: false, message: "Config loaded" },
    { name: "Memory DB", ok: true, is_warning: false, message: "SQLite accessible" },
    { name: "Memory workspace", ok: true, is_warning: false, message: "Dirs created" },
    { name: "Skills", ok: true, is_warning: false, message: "3 skills loaded" },
    { name: "Gateway", ok: false, is_warning: true, message: "No gateway tokens configured" },
    { name: "Vision tools", ok: false, is_warning: true, message: "Gemini CLI not found" },
  ],
  list_agents: async () => [],
};
```

**`data-testid` attributes for all new surfaces:**

| Screen / Element | `data-testid` |
|---|---|
| Wizard overlay | `onboard-overlay` |
| Wizard step bar | `onboard-step-bar` |
| Screen 1 path chip | `onboard-workspace-path` |
| Screen 1 change button | `onboard-workspace-change` |
| Screen 2 provider radio | `onboard-provider-radio` |
| Screen 2 api_key input | `onboard-api-key` |
| Screen 2 test connection button | `onboard-test-connection` |
| Screen 2 test result | `onboard-test-result` |
| Screen 3 Card A | `onboard-tools-card-a` |
| Screen 3 Card B | `onboard-tools-card-b` |
| Screen 4 username input | `onboard-username` |
| Screen 4 finish CTA | `onboard-finish-cta` |
| Memory nav item | `nav-memory` |
| Memory MEMORY.md textarea | `memory-md-textarea` |
| Memory save button | `memory-md-save` |
| Memory search box | `memory-search-input` |
| Skills nav item | `nav-skills` |
| Skills list | `skills-list` |
| Skills new button | `skills-new-btn` |
| Status nav item | `nav-status` |
| Status doctor run button | `status-run-doctor` |
| Status doctor results | `status-doctor-results` |
| Status run wizard link | `status-run-wizard` |
| Settings re-run wizard link | `settings-rerun-wizard` |

**Playwright test flows (`tests/e2e/onboarding.spec.cjs`):**
1. Full wizard happy path: `get_app_state` returns `initial_view: "onboarding"` → wizard overlay shown → Screen 1 (path accepted) → Screen 2 (mock probe returns success) → Screen 3 (Card A clicked) → Screen 4 (finish CTA) → wizard dismissed, Chat visible.
2. Wizard auth failure: probe returns `error_type: "auth_failure"` → red error banner shown → Continue locked → user fixes key → probe returns success → Continue unlocks.
3. Network failure: probe returns `error_type: "network_failure"` → amber error banner shown → correct message text.
4. Model unavailable (404): probe returns `error_type: "model_unavailable"` → error shows "Model not found" (not "Invalid API key").
5. Wizard re-entry from Settings: Settings view → `data-testid="settings-rerun-wizard"` click → wizard overlay shown with pre-filled fields.
6. Memory view: `nav-memory` click → MEMORY.md textarea populated → edit + save → `write_memory_md` invoked → search box → results appear.
7. Skills view: `nav-skills` click → skills list rendered with at least 1 item → select item → content shown.
8. Status view: `nav-status` click → `status-run-doctor` click → `status-doctor-results` populated with 6 rows.

## Architecture

### ASCII diagram

```
+-------------------------------------------------------------------------+
|  Desktop App (Tauri 2.x process — main window)                          |
|                                                                         |
|  +---+-------------------------------------------+-----------------+   |
|  |Nav|  Main content area (SPA routing)           |  (Settings:     |   |
|  |rai|                                            |  two-column     |   |
|  |l  |  Wizard overlay (full-screen, when         |  sub-nav)       |   |
|  |   |  initial_view=="onboarding")               |                 |   |
|  |Cha|  +------------------------------------+    |                 |   |
|  | t |  | Screen 1: Workspace               |    |                 |   |
|  |   |  | Screen 2: AI Provider + Test      |    |                 |   |
|  |Mem|  | Screen 3: Permissions Consent     |    |                 |   |
|  |ory|  | Screen 4: Identity + Finish       |    |                 |   |
|  |   |  +------------------------------------+    |                 |   |
|  |Ski|  Chat view / Memory view / Skills view /   |                 |   |
|  |lls|  Status view (after wizard dismissed)      |                 |   |
|  |   |                                            |                 |   |
|  |Sta|  CSP: default-src 'self' (unchanged)       |                 |   |
|  | ts|  No LLM origin in connect-src              |                 |   |
|  |   |                                            |                 |   |
|  |Set+--------------------------------------------+-----------------+   |
|  +---+                                                                  |
|       invoke(command, args) --> Rust backend                            |
+-------------------------------------------------------------------------+
                                     |
                        +------------v-----------+
                        |  AppCore (state.rs)     |
                        |  agent: Agent           |
                        |  turn: tokio::Mutex<    |
                        |    Turn { ctx, session, |
                        |           tools_enabled }|
                        +------------+-----------+
                                     | in-process call
                                     v
                +---------------------------------------------+
                |  open_assistant [lib] (src/lib.rs)          |
                |                                             |
                |  config::{load, save}  (all writes bypass   |
                |    config::set(); use load/mutate/save)      |
                |  config::Config.tools.enabled  (NEW field)  |
                |                                             |
                |  core::agent::Agent::process()              |
                |  core::memory::MemoryWorkspace              |
                |  memory::store::MemoryStore  (SQLite FTS5)  |
                |  skills::engine::SkillEngine::load_builtin()|
                |  core::subagent::SubAgentOrchestrator       |
                |    (definitions only — no spawn)            |
                |  tools::vision::check() (with timeout)      |
                |                                             |
                |  [NOT CALLED from desktop this phase]       |
                |  core::subagent::execute_subagent() STUB    |
                |  core::workflows::WorkflowEngine::execute() |
                |  core::checkpoint::CheckpointStore (mem)    |
                |  core::plugins::PluginMarketplace::install()|
                +---------------------------------------------+

Shared on disk:  ~/.openassistant/
  config.yaml   (now with [tools] section)
  MEMORY.md
  memory/YYYY-MM-DD.md
  memory.db
  skills/
  .claude/agents/
  .claude/plugins/
```

### Complete command surface table

| Command | Module | New/Existing | Returns | Backed by |
|---|---|---|---|---|
| `send_message` | `chat` | Existing | `Result<Message>` | `Agent::process` |
| `get_history` | `chat` | Existing | `Result<Vec<Message>>` | session.messages |
| `get_status` | `chat` | Extended | `Result<StatusResponse>` | + `MemoryStore::count`, `MemoryWorkspace::read_long_term` |
| `clear_conversation` | `chat` | Existing | `Result<()>` | session reset |
| `get_config` | `settings` | Existing | `Result<ConfigDto>` | `config::load` |
| `save_config` | `settings` | Existing | `Result<()>` | load/mutate/save |
| `set_tools_enabled` | `settings` | Existing | `Result<()>` | AppCore + `cfg.tools.enabled` |
| `get_app_state` | `onboarding` | NEW | `Result<AppState>` | `config::load` |
| `save_onboarding_config` | `onboarding` | NEW | `Result<()>` | load/mutate/save (all wizard fields) |
| `get_memory_md` | `memory` | NEW | `Result<String>` | `MemoryWorkspace::read_long_term` |
| `write_memory_md` | `memory` | NEW | `Result<()>` | `MemoryWorkspace::write_long_term` |
| `get_today_note` | `memory` | NEW | `Result<String>` | `MemoryWorkspace::read_today` |
| `search_memory_files` | `memory` | NEW | `Result<Vec<[String;2]>>` | `MemoryWorkspace::search_files` |
| `list_skills` | `skills` | NEW | `Result<Vec<SkillDto>>` | `SkillEngine::load_builtin` + `load_from_dir` |
| `read_skill` | `skills` | NEW | `Result<String>` | `SkillEngine::get` |
| `create_skill` | `skills` | NEW | `Result<()>` | `tokio::fs::write` |
| `probe_connection` | `system` | NEW | `Result<ProbeResult>` | `reqwest::Client` direct (NOT `Agent::process`) |
| `check_path_writable` | `system` | NEW | `Result<bool>` | `tokio::fs::create_dir_all` + write probe |
| `pick_data_dir` | `system` | NEW | `Result<Option<String>>` | `tauri-plugin-dialog` `blocking_pick_folder` |
| `run_doctor` | `system` | NEW | `Result<Vec<DiagnosticResult>>` | 6 real checks (config, memory db, workspace, skills, gateway, vision) |
| `list_agents` | `system` | NEW | `Result<Vec<AgentDto>>` | `SubAgentOrchestrator::load_definitions` (read-only) |

**Total: 21 commands** (7 from v1, 14 new).

### Cargo.toml additions (`src-tauri/Cargo.toml`)

```toml
tauri-plugin-dialog = "2"    # folder/file picker
tauri-plugin-opener = "2"    # open external URLs via OS shell
reqwest = { version = "0.12", features = ["json"] }  # probe_connection (direct dep, not just transitive)
```

### Config change (`src/config/mod.rs`)

```toml
# Added to config.yaml structure:
[tools]
enabled = false
```

```rust
// Added to src/config/mod.rs:
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub enabled: bool,
}
impl Default for ToolsConfig {
    fn default() -> Self { Self { enabled: false } }
}

// In Config struct:
pub tools: ToolsConfig,
```

## Testing

### Rust unit tests (all platforms, CI-friendly)

All tests live in `#[cfg(test)]` modules within each command sub-module.

**`commands/onboarding.rs` tests:**
- `test_save_onboarding_config_round_trip`: construct `OnboardingDto` with all fields set, call the command logic (using a temp dir for config path), reload config, assert all fields persisted. Asserts: `data_dir`, `user_name` trimmed to 64 chars, `provider`, `model`, `api_base`, `tools.enabled == true`, `api_key` non-empty, `skills.dirs[0]` = `{data_dir}/skills`.
- `test_save_onboarding_config_empty_api_key_preserved`: when `dto.api_key` is empty, the existing config `api_key` is NOT overwritten.
- `test_save_onboarding_config_user_name_defaults_to_friend`: when `dto.user_name` is `None`, config `general.user_name` is `"friend"`.

**`commands/system.rs` tests:**
- `test_probe_connection_success`: mock HTTP 200 → `ProbeResult { ok: true }`.
- `test_probe_connection_auth_failure`: mock HTTP 401 → `ProbeResult { error_type: Some("auth_failure") }`.
- `test_probe_connection_model_unavailable`: mock HTTP 404 → `ProbeResult { error_type: Some("model_unavailable") }` with message containing "Model not found", NOT "Invalid API key".
- `test_probe_connection_network_failure`: timeout/DNS error → `ProbeResult { error_type: Some("network_failure") }`.
- `test_run_doctor_skills_count`: `run_doctor` result contains a "Skills" entry with `ok: true` and message containing "3" (uses `load_builtin`, not `default()`).
- `test_check_path_writable_valid`: writable temp dir → `Ok(true)`.

**`commands/settings.rs` tests (extending existing):**
- `test_tools_enabled_persists`: `set_tools_enabled(true)` → `config::save` → reload → `cfg.tools.enabled == true`.

**`commands/skills.rs` tests:**
- `test_list_skills_builtin_count`: `list_skills` returns at least 3 items (the three built-ins).

### Playwright E2E tests (`tests/e2e/`)

**`onboarding.spec.cjs`** — 8 flows documented in D13 above. Runs in headless Chromium against the static dev server. All assertions use `data-testid` selectors. All Tauri commands use `window.__MOCK_BACKEND__`.

**Extended `chat.spec.cjs`** — smoke test that `get_app_state` returning `initial_view: "chat"` keeps the wizard overlay hidden.

**`memory.spec.cjs`** — MEMORY.md content shown in textarea; save button invokes `write_memory_md`; search results appear.

**`skills.spec.cjs`** — skills list rendered; selecting a skill shows content; "New Skill" modal opens and `create_skill` is invoked.

**`status.spec.cjs`** — Status panel renders; "Run diagnostics" button triggers `run_doctor`; 6 rows rendered; amber rows for warning results; no "Run" button on agent definitions.

### Native smoke (Windows + Linux only)

Extend `tests/e2e/` native `tauri-driver` smoke to cover: app launch with empty api_key → wizard overlay visible (`onboard-overlay` element present); wizard completes (via mocked commands) → chat view visible.

## Risks & Mitigations

| # | Risk | Mitigation |
|---|---|---|
| R1 | **Stub surfacing** — `execute_subagent` (`subagent.rs:267-283`) returns fabricated success; `WorkflowEngine::execute` (`workflows.rs:141-161`) marks all steps completed with fake strings; checkpoint restore loses all data on app close. | Non-Goals table names each with source-line citations. No `spawn_agent`, `run_workflow`, or `restore_checkpoint` command. Agents view is explicitly read-only with a banner. Capability honesty table in the spec gates future contributors. |
| R2 | **`tools.enabled` session-only gap** — existing `set_tools_enabled` writes only in-memory state; consent from Screen 3 is lost on restart. | Add `ToolsConfig { enabled: bool }` to `Config` struct (D4). `save_onboarding_config` writes `cfg.tools.enabled`. `build_core()` reads `cfg.tools.enabled` on startup. `#[cfg(test)]` round-trip test verifies persistence. |
| R3 | **`config::set()` silent no-op** — unknown keys hit `_ => tracing::warn!` arm and write nothing. | All desktop commands use `load/mutate/save` exclusively. Non-Goals table names this. The single `generate_handler!` comment warns future contributors. |
| R4 | **Skills count = 0 in Doctor** — `SkillEngine::default().count()` returns 0 because `default()` does not call `load_builtin()`. This is a CLI bug that would be replicated. | `run_doctor` uses `SkillEngine::load_builtin()?.count()` (returns 3). `test_run_doctor_skills_count` asserts count ≥ 3. |
| R5 | **Vision check hangs on Windows** — `tools::vision::check()` spawns `gemini --skip-trust whoami`; if `gemini.exe` is not on PATH, the process spawn may be slow to fail. | 200ms timeout via `tokio::time::timeout`. Timeout and binary-not-found both render as `is_warning: true` (amber), not a red error. |
| R6 | **Plugin enable state reset on restart** — `set_enabled()` is in-memory only; restart resets all toggles. | Plugin enable/disable toggle is NOT exposed this change. `PluginMarketplace` is not added to `AppCore` state. Deferred to when `set_enabled` persists to `plugin.json`. |
| R7 | **Wizard gate conflict with existing `boot()` flow** — the existing `app.js` `boot()` calls `get_config` and checks `api_key_set`; adding `get_app_state` creates a second routing check that can desync. | Replace the existing `api_key_set` branch in `boot()` with a single call to `get_app_state`; use `initial_view` as the sole routing signal. Remove the duplicate `api_key_set` check. One code path, one source of truth. |
| R8 | **`probe_connection` side effects** — if implemented via `Agent::process`, would write daily notes and mutate `UserModel` before the user sends a real message. | `probe_connection` uses `reqwest::Client::new()` directly (D3). The `Agent::process` code path is never touched. `#[cfg(test)]` tests use a mock HTTP server (not the agent). |
| R9 | **`MemoryStore` per-call open cost** — `MemoryStore::open_default().await` opens and closes a SQLite connection per `get_status` call (~5ms). | Acceptable for v1 (status is fetched infrequently). Adding `Mutex<Option<MemoryStore>>` to `AppCore` is a P1 optimization, noted not done. |
| R10 | **`FilePath.to_string()` pitfall** — calling `.to_string()` on the `tauri_plugin_dialog::FilePath` enum serializes the variant name, not the path. | Use `.as_path().map(|p| p.display().to_string())` everywhere. This is enforced in `pick_data_dir` implementation (D5) and noted in code comments. |
| R11 | **Wizard/Settings drift** — wizard and Settings write the same config fields through different code paths; validation can diverge. | Both call `save_onboarding_config` and `save_config` respectively; both use the identical `load/mutate/save` pattern. `save_config` is not changed to call `save_onboarding_config`; they are parallel but share the same underlying `config::save` primitive. Field coverage is documented in D10 and D6 respectively. |
| R12 | **`opener:allow-open-url` URL wildcard** — `https://*` allows opening any URL if the webview is compromised. | Scoped to three specific domains (`openrouter.ai`, `openai.com`, `platform.openai.com`) per D12. |
| R13 | **Single `invoke_handler` invariant** — a second `.invoke_handler()` call silently discards all previously registered commands. | Guard comment added in `lib.rs`: `// SINGLE invoke_handler — Tauri keeps only the LAST registration. Add new commands here.` |
| R14 | **Windows path display with forward slashes** — Rust's `PathBuf` on Windows uses backslashes; hardcoded string formatting may produce forward slashes in the UI. | Use `std::path::Path::display()` for all path rendering (D5). `pick_data_dir` returns `p.display().to_string()` which respects the OS separator. |
| R15 | **Gateway token expectations** — saving Discord/Telegram tokens in Settings > Channels implies they work; the gateway is mostly placeholder. | Gateway token section has a visible "experimental" callout: "Gateway channel integration is experimental — the messaging server is not yet fully operational." No connection-status display, no connect/test button for gateway tokens. |
