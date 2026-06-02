//! openAssistant desktop — Tauri 2.x shell that reuses the `open_assistant`
//! agent core in-process. See openspec change `add-desktop-app`.

mod commands;
mod state;

use open_assistant::core::agent::Agent;
use state::AppCore;
use tauri::Manager;

/// Build the managed core: load config, construct the agent pointed at the
/// configured data dir, with tool execution OFF by default (the packaged app
/// must not hand the model ungated shell/file access without consent).
fn build_core() -> AppCore {
    let cfg = tauri::async_runtime::block_on(open_assistant::config::load()).unwrap_or_default();
    let agent = Agent::new(cfg.model.model)
        .with_workspace(cfg.general.data_dir)
        .with_tools_enabled(cfg.tools.enabled);
    AppCore::new(agent)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
        .invoke_handler(tauri::generate_handler![
            commands::send_message,
            commands::get_history,
            commands::get_status,
            commands::clear_conversation,
            commands::get_config,
            commands::save_config,
            commands::set_tools_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
