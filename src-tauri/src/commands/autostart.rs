//! Launch-at-system-startup commands. Thin wrappers over `tauri-plugin-autostart`'s
//! `AutoLaunchManager` so the static frontend toggles autostart through the same
//! `invoke` path as every other command (no JS plugin bindings / capability needed).
//!
//! The OS registration (Windows `HKCU\…\Run`, macOS LaunchAgent, Linux
//! `~/.config/autostart`) is the source of truth; we mirror the desired state into
//! `config.desktop.launch_at_startup` only so the Settings toggle can render the
//! intent without an async probe.

use open_assistant::config;
use tauri_plugin_autostart::ManagerExt;

/// Authoritative OS autostart state (true = registered to launch at login).
#[tauri::command(rename_all = "snake_case")]
pub async fn autostart_is_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Enable/disable launch-at-startup: write the OS registration, persist the desired
/// state, and latch `autostart_initialized` so setup() never overrides a deliberate
/// user choice. Returns the authoritative post-change OS state.
#[tauri::command(rename_all = "snake_case")]
pub async fn set_launch_at_startup(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    {
        let mgr = app.autolaunch();
        if enabled {
            mgr.enable().map_err(|e| e.to_string())?;
        } else {
            mgr.disable().map_err(|e| e.to_string())?;
        }
    }

    let mut cfg = config::load().await.map_err(|e| e.to_string())?;
    cfg.desktop.launch_at_startup = enabled;
    cfg.desktop.autostart_initialized = true; // user has taken control
    config::save(&cfg).await.map_err(|e| e.to_string())?;

    Ok(app.autolaunch().is_enabled().unwrap_or(enabled))
}
