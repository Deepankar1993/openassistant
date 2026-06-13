//! openAssistant desktop — Tauri 2.x shell that reuses the `open_assistant`
//! agent core in-process. See openspec changes `add-desktop-app` and
//! `add-desktop-onboarding-options`.
//!
//! ── CAPABILITY HONESTY TABLE ────────────────────────────────────────────────
//! The desktop app surfaces ONLY core features that are verified end-to-end. The
//! following are STILL STUBS in the core and MUST NOT be given a working UI
//! affordance (no "Run"/"Spawn"/"Install"/"Activate" button). Read-only listing
//! is allowed where noted.
//!
//!   Feature                 | Why not surfaced                              | Source
//!   ------------------------|-----------------------------------------------|------------------------------
//!   Sub-agent execution     | execute_subagent() returns a placeholder       | core/subagent.rs:267-283
//!   Plugin marketplace      | Marketplace source always Err(...)             | core/plugins.rs:216
//!   Skill activation toggle | activate_skill() not read by Agent::process    | skills/engine.rs
//!   plan/perm handlers      | return placeholder text                        | core/agent.rs
//!
//! `list_agents` (read-only definitions) is the only sub-agent surface allowed.
//!
//! ── NOW REAL IN THE CORE (CLI-first; desktop surface deferred, see openspec
//!    change `complete-core-features-and-integrations`) ─────────────────────────
//! These are wired end-to-end on the CLI but intentionally NOT yet surfaced in
//! the desktop this cycle; a later desktop change can add UI against them:
//!   Workflow execution  — real LLM steps + persistence (core/workflows.rs).
//!   Checkpoint restore  — persistent SQLite + SHA-256-guarded restore (core/checkpoint.rs).
//!   Self-update         — real git source update via `openassistant update`
//!                         (a desktop button still needs tauri-plugin-updater + an endpoint).
//!   Gateway channels    — WebChat/Discord/Telegram/Slack all run the real
//!                         Agent::process loop (gateway/); Slack needs a public URL.
//!   goal_deliberate     — real per-role LLM deliberation + persisted goals/subgoals.
//! ────────────────────────────────────────────────────────────────────────────

mod commands;
mod state;

use open_assistant::core::agent::Agent;
use state::AppCore;
use tauri::Manager;

/// Build the managed core: load config, construct the agent pointed at the
/// configured data dir, honoring the persisted tool-execution posture.
fn build_core() -> AppCore {
    let cfg = tauri::async_runtime::block_on(open_assistant::config::load()).unwrap_or_default();
    let data_dir = cfg.general.data_dir.clone();
    let persona = open_assistant::core::persona::Persona::load_or_default(&data_dir);
    // The desktop app is the trusted local operator → lifecycle hooks may fire.
    let agent = Agent::new(cfg.model.model)
        .with_workspace(data_dir)
        .with_tools_enabled(cfg.tools.enabled)
        .operator();
    AppCore::new(agent, persona)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Plugins registered before .setup(): logging, then dialog (folder
        // picker) and opener (external provider-docs links).
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.manage(build_core());
            Ok(())
        })
        // SINGLE invoke_handler — Tauri keeps only the LAST registration.
        // Add ALL new commands here. Never call .invoke_handler() a second time.
        .invoke_handler(tauri::generate_handler![
            // chat
            commands::chat::send_message,
            commands::chat::send_message_stream,
            commands::chat::get_history,
            commands::chat::get_status,
            commands::chat::clear_conversation,
            commands::chat::new_conversation,
            commands::chat::list_conversations,
            commands::chat::switch_conversation,
            commands::chat::delete_conversation,
            // settings
            commands::settings::get_config,
            commands::settings::save_config,
            commands::settings::save_full_config,
            commands::settings::set_tools_enabled,
            // persona
            commands::persona::get_persona,
            commands::persona::save_persona,
            // onboarding
            commands::onboarding::get_app_state,
            commands::onboarding::probe_connection,
            commands::onboarding::check_path_writable,
            commands::onboarding::pick_data_dir,
            commands::onboarding::save_onboarding_config,
            // memory
            commands::memory::get_memory_md,
            commands::memory::write_memory_md,
            commands::memory::get_today_note,
            commands::memory::search_memory_files,
            commands::memory::list_user_facts,
            commands::memory::add_user_fact,
            commands::memory::update_user_fact,
            commands::memory::delete_user_fact,
            // skills
            commands::skills::list_skills,
            commands::skills::read_skill,
            commands::skills::create_skill,
            // system
            commands::system::run_doctor,
            commands::system::gateway_readiness,
            commands::system::open_external_url,
            commands::system::list_agents,
            // gateway control
            commands::gateway::gateway_start,
            commands::gateway::gateway_stop,
            commands::gateway::gateway_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
