//! Onboarding-wizard commands.

use crate::state::AppCore;
use open_assistant::config;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri::State;

/// First-paint routing hint so the frontend never flashes Chat before the wizard.
#[derive(Debug, Serialize)]
pub struct AppStateDto {
    pub initial_view: String, // "onboarding" | "chat"
    pub api_key_set: bool,
    pub user_name: String,
    pub data_dir: String,
}

/// Result of a connection probe against the LLM endpoint.
#[derive(Debug, Serialize)]
pub struct ProbeResultDto {
    pub ok: bool,
    pub latency_ms: u64,
    pub error_type: Option<String>, // "auth_failure" | "model_unavailable" | "network_error"
    pub error_message: Option<String>,
}

/// All fields collected by the 4-screen wizard, saved atomically.
#[derive(Debug, Deserialize)]
pub struct OnboardingDto {
    pub data_dir: String,
    pub provider: String,
    pub model: String,
    pub api_base: String,
    pub api_key: String,
    pub tools_enabled: bool,
    pub user_name: Option<String>,
    pub skills_dirs: Vec<String>,
}

/// Map a non-2xx HTTP status from the probe to a stable error category. Kept
/// pure (no I/O) so the mapping is unit-tested without a live endpoint. A 404
/// is `model_unavailable` (not `auth_failure`) so a missing default model never
/// looks like a bad key.
fn classify_error_status(code: u16) -> &'static str {
    match code {
        401 | 403 => "auth_failure",
        404 => "model_unavailable",
        _ => "network_error",
    }
}

/// Route to the wizard on first run (empty api_key) or to Chat otherwise.
#[tauri::command]
pub async fn get_app_state() -> Result<AppStateDto, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let api_key_set = !cfg.model.api_key.trim().is_empty();
    Ok(AppStateDto {
        initial_view: if api_key_set { "chat".into() } else { "onboarding".into() },
        api_key_set,
        user_name: cfg.general.user_name,
        data_dir: cfg.general.data_dir,
    })
}

/// Probe the LLM endpoint with a minimal request. Uses `reqwest` directly — NOT
/// `Agent::process` — so the user's very first interaction does not write a
/// daily note or mutate the `UserModel`. Distinguishes auth / model / network
/// failures so a missing default model never looks like a bad key.
#[tauri::command]
pub async fn probe_connection(
    api_key: String,
    api_base: String,
    model: String,
) -> Result<ProbeResultDto, String> {
    if api_key.trim().is_empty() {
        return Ok(ProbeResultDto {
            ok: false,
            latency_ms: 0,
            error_type: Some("auth_failure".into()),
            error_message: Some("No API key provided.".into()),
        });
    }

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": "hi" }],
        "max_tokens": 1,
    });
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));

    let started = Instant::now();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis() as u64;

    match resp {
        Ok(r) if r.status().is_success() => Ok(ProbeResultDto {
            ok: true,
            latency_ms,
            error_type: None,
            error_message: None,
        }),
        Ok(r) => {
            let code = r.status().as_u16();
            let error_type = classify_error_status(code);
            let msg = match error_type {
                "auth_failure" => "Invalid API key.".to_string(),
                "model_unavailable" => format!("Model `{model}` not found."),
                _ => format!("Endpoint returned HTTP {code}."),
            };
            Ok(ProbeResultDto {
                ok: false,
                latency_ms,
                error_type: Some(error_type.into()),
                error_message: Some(msg),
            })
        }
        Err(e) => Ok(ProbeResultDto {
            ok: false,
            latency_ms,
            error_type: Some("network_error".into()),
            error_message: Some(e.to_string()),
        }),
    }
}

/// Create the data dir (+ `memory/` and `skills/` subdirs) and verify it is
/// writable. Returns Ok(false) (not Err) for an unwritable path so the UI can
/// render an inline state for both outcomes.
#[tauri::command]
pub async fn check_path_writable(path: String) -> Result<bool, String> {
    let base = std::path::PathBuf::from(&path);
    if tokio::fs::create_dir_all(base.join("memory")).await.is_err() {
        return Ok(false);
    }
    if tokio::fs::create_dir_all(base.join("skills")).await.is_err() {
        return Ok(false);
    }
    let probe = base.join(".probe");
    match tokio::fs::write(&probe, b"ok").await {
        Ok(_) => {
            let _ = tokio::fs::remove_file(&probe).await;
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

/// Native folder picker for the data dir. Runs the blocking dialog off the async
/// executor; converts `FilePath` via `.as_path()` (not the enum's `to_string`).
#[tauri::command]
pub async fn pick_data_dir(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        app.dialog().file().blocking_pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(picked.and_then(|fp| fp.as_path().map(|p| p.display().to_string())))
}

/// Save all wizard fields at once via the full load/mutate/save path.
#[tauri::command]
pub async fn save_onboarding_config(
    state: State<'_, AppCore>,
    dto: OnboardingDto,
) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    let mut cfg = config::load().await.map_err(|e| e.to_string())?;

    cfg.general.data_dir = dto.data_dir.trim().to_string();
    cfg.model.provider = dto.provider.trim().to_string();
    cfg.model.model = dto.model.trim().to_string();
    cfg.model.api_base = dto.api_base.trim().to_string();
    if !dto.api_key.trim().is_empty() {
        cfg.model.api_key = dto.api_key.trim().to_string();
    }
    cfg.tools.enabled = dto.tools_enabled;
    cfg.general.user_name = dto
        .user_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "friend".into());
    let dirs: Vec<String> = dto.skills_dirs.into_iter().filter(|d| !d.trim().is_empty()).collect();
    if !dirs.is_empty() {
        cfg.skills.dirs = dirs;
    }

    config::save(&cfg).await.map_err(|e| e.to_string())?;
    turn.agent.tools_enabled = cfg.tools.enabled;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_status_classification() {
        assert_eq!(classify_error_status(401), "auth_failure");
        assert_eq!(classify_error_status(403), "auth_failure");
        // 404 must be distinct so a missing default model is not read as a bad key.
        assert_eq!(classify_error_status(404), "model_unavailable");
        assert_eq!(classify_error_status(500), "network_error");
        assert_eq!(classify_error_status(429), "network_error");
    }

    #[test]
    fn onboarding_dto_deserializes_from_frontend_payload() {
        // Locks the frontend↔backend wire contract (snake_case field names).
        let json = serde_json::json!({
            "data_dir": "C:\\Users\\x\\.openassistant",
            "provider": "openrouter",
            "model": "openrouter/owl-alpha",
            "api_base": "https://openrouter.ai/api/v1",
            "api_key": "sk-test",
            "tools_enabled": false,
            "user_name": "friend",
            "skills_dirs": ["C:\\Users\\x\\.openassistant\\skills"]
        });
        let dto: OnboardingDto = serde_json::from_value(json).expect("deserialize");
        assert_eq!(dto.provider, "openrouter");
        assert!(!dto.tools_enabled);
        assert_eq!(dto.skills_dirs.len(), 1);
        assert_eq!(dto.user_name.as_deref(), Some("friend"));
    }
}
