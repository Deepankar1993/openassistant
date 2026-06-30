//! In-app auto-update via `tauri-plugin-updater`, driven from Rust.
//!
//! The updater checks the GitHub-releases `latest.json` manifest for a newer,
//! **signature-verified** version and applies it in place — no installer dialog,
//! no "uninstall before installing" prompt. It only touches the program files;
//! the user's `~/.openassistant` data dir (config.yaml / API keys / memory) is
//! never involved, so settings always persist across updates.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Result of an update check — availability plus the target version/notes,
/// without downloading anything.
#[derive(Debug, Serialize)]
pub struct UpdateStatus {
    pub available: bool,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
}

/// Check the configured endpoint for a newer signed release.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateStatus, String> {
    let current = app.package_info().version.to_string();
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateStatus {
            available: true,
            current_version: current,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
        }),
        Ok(None) => Ok(UpdateStatus {
            available: false,
            current_version: current,
            version: None,
            notes: None,
        }),
        Err(e) => Err(format!("update check failed: {e}")),
    }
}

/// Download + install the available update in place, then relaunch.
/// Errors (and returns to the caller) when no update is available; on success it
/// restarts and never returns.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("update check failed: {e}"))?
        .ok_or_else(|| "no update available".to_string())?;

    update
        .download_and_install(|_chunk: usize, _total: Option<u64>| {}, || {})
        .await
        .map_err(|e| format!("update install failed: {e}"))?;

    // Relaunch into the freshly-installed version. `restart` diverges (`!`),
    // so its type coerces to this function's `Result` return.
    app.restart()
}
