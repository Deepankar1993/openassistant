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
//!   Self-update         — real git source update via `openassistant update`; the
//!                         DESKTOP now ships tauri-plugin-updater (commands::updater::
//!                         {check_for_update,install_update}) against the GitHub-releases
//!                         latest.json — one-click, signature-verified, no installer prompt.
//!   Gateway channels    — WebChat/Discord/Telegram/Slack all run the real
//!                         Agent::process loop (gateway/); Slack needs a public URL.
//!   goal_deliberate     — real per-role LLM deliberation + persisted goals/subgoals.
//!   Always-on desktop   — system-tray background running (close→hide to tray),
//!                         gateway auto-start + keep-alive, launch-at-login
//!                         (tauri-plugin-autostart), and a single-instance guard.
//! ────────────────────────────────────────────────────────────────────────────

mod commands;
mod state;

use open_assistant::core::agent::Agent;
use state::AppCore;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;

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
        // single-instance MUST be first: a 2nd launch focuses the running window
        // instead of spawning a duplicate process (which would fight over the
        // gateway port). The callback runs in the ALREADY-running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        // Folder picker (onboarding) + scoped external-link opening (provider docs).
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // In-app auto-update: one-click, signature-verified, no installer prompt.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Launch openAssistant at system startup (always-on). Toggleable in Settings.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostarted"]),
        ))
        // Close-to-tray: the window "X" hides it (the app + gateway keep running in
        // the tray). Only a tray "Quit" (app.exit) truly exits. Scoped to "main".
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.manage(build_core());

            // ── System tray (run-in-background control) ──────────────────────
            let show_i = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Hide Window", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let start_i = MenuItem::with_id(app, "gateway_start", "Start Gateway", true, None::<&str>)?;
            let stop_i = MenuItem::with_id(app, "gateway_stop", "Stop Gateway", true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit openAssistant", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&show_i, &hide_i, &sep1, &start_i, &stop_i, &sep2, &quit_i],
            )?;

            let mut tray = TrayIconBuilder::with_id("main-tray")
                .tooltip("openAssistant")
                .menu(&menu)
                // Left-click shows the window (menu opens on right-click).
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.unminimize();
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "hide" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.hide();
                        }
                    }
                    // Gateway start is async + behind a tokio Mutex in AppCore.
                    "gateway_start" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<AppCore>();
                            let mut g = state.gateway.lock().await;
                            if g.as_ref().map_or(false, |h| h.is_running()) {
                                return;
                            }
                            match open_assistant::config::load().await {
                                Ok(cfg) => match open_assistant::gateway::start_gateway_handle(cfg).await {
                                    Ok(h) => {
                                        log::info!("tray: gateway started on {}", h.addr);
                                        *g = Some(h);
                                    }
                                    Err(e) => log::error!("tray: gateway start failed: {e}"),
                                },
                                Err(e) => log::error!("tray: config load failed: {e}"),
                            }
                        });
                    }
                    "gateway_stop" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(h) = app.state::<AppCore>().gateway.lock().await.take() {
                                h.stop();
                            }
                        });
                    }
                    // Quit REALLY exits: abort the gateway task, then exit the loop.
                    "quit" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(h) = app.state::<AppCore>().gateway.lock().await.take() {
                                h.stop();
                            }
                            app.exit(0);
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.unminimize();
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                });
            // The bundle icon (now the owl) — set only if present so we never panic.
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            let _tray = tray.build(app)?;

            // ── Always-on bring-up (off-thread; never fatal) ─────────────────
            // First-run autostart registration + auto-start the gateway when
            // onboarded. Errors are logged, never propagated (no panic on a busy
            // port or a restricted environment).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = sync_autostart_first_run(&handle).await {
                    log::warn!("autostart first-run sync skipped: {e}");
                }
                if let Err(e) = autostart_gateway(&handle).await {
                    log::info!("gateway autostart skipped: {e}");
                }
            });

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
            commands::settings::get_providers,
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
            // schedules (read-only listings of cron / standing-orders / watchers / brief)
            commands::schedules::list_cron_jobs,
            commands::schedules::list_standing_orders,
            commands::schedules::list_watchers,
            commands::schedules::get_brief_settings,
            commands::schedules::get_schedules_overview,
            // in-app auto-update (one-click, signature-verified, no installer prompt)
            commands::updater::check_for_update,
            commands::updater::install_update,
            // launch-at-startup (always-on)
            commands::autostart::autostart_is_enabled,
            commands::autostart::set_launch_at_startup,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // On a real Quit, abort the in-process gateway task so it isn't leaked
            // (the window-hide path keeps it running on purpose).
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppCore>() {
                    // try_lock: at exit nothing should contend (the tray "Quit"
                    // path releases the lock before exit). If it's briefly held,
                    // skip — the process is terminating and the OS reaps the tasks.
                    if let Ok(mut g) = state.gateway.try_lock() {
                        if let Some(h) = g.take() {
                            h.stop();
                        }
                    }
                }
            }
        });
}

/// First-run only: push `config.desktop.launch_at_startup` into the OS autostart
/// registration, then latch `autostart_initialized` so the user owns the state
/// afterward. Best-effort. Debug builds skip registration so `cargo tauri dev`
/// never registers a throwaway debug binary at login.
async fn sync_autostart_first_run(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let mut cfg = open_assistant::config::load().await?;
    if cfg.desktop.autostart_initialized {
        return Ok(());
    }
    if !cfg!(debug_assertions) {
        let mgr = app.autolaunch();
        let currently = mgr.is_enabled().unwrap_or(false);
        if cfg.desktop.launch_at_startup && !currently {
            if let Err(e) = mgr.enable() {
                log::warn!("autostart enable failed (continuing): {e}");
            }
        } else if !cfg.desktop.launch_at_startup && currently {
            let _ = mgr.disable();
        }
    }
    cfg.desktop.autostart_initialized = true;
    open_assistant::config::save(&cfg).await?;
    Ok(())
}

/// Start the in-process gateway on launch when configured AND onboarded; store the
/// handle in `AppCore` so a later Quit can `.stop()` it. Never panics: a busy port
/// or any error is logged and bring-up is abandoned.
async fn autostart_gateway(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let cfg = open_assistant::config::load().await?;
    if !cfg.desktop.gateway_autostart {
        anyhow::bail!("desktop.gateway_autostart is false");
    }
    // Onboarding gate: no API key ⇒ the app is on the wizard, not chat.
    if cfg.model.api_key.trim().is_empty() {
        anyhow::bail!("onboarding incomplete (no api key)");
    }
    let state = app.state::<AppCore>();
    let mut g = state.gateway.lock().await;
    if g.as_ref().map_or(false, |h| h.is_running()) {
        return Ok(());
    }
    match open_assistant::gateway::start_gateway_handle(cfg).await {
        Ok(h) => {
            log::info!("gateway auto-started on http://{}", h.addr);
            *g = Some(h);
        }
        // Port busy (e.g. a 2nd instance / CLI gateway) — log + continue. The
        // up-front bind in start_gateway_handle prevents any double-bind.
        Err(e) => log::warn!("gateway auto-start failed (port busy?): {e}"),
    }
    Ok(())
}
