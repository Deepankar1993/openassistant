//! Settings commands: read/write the full config (always via load/mutate/save,
//! never `config::set()`, whose allowlist silently drops most keys).

use super::mask_key;
use crate::state::AppCore;
use open_assistant::config;
use open_assistant::core::agent::Agent;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Config surfaced to the Settings view. Secrets are never sent in clear — only
/// a masked preview plus a boolean indicating whether one is set.
#[derive(Debug, Serialize)]
pub struct ConfigDto {
    // Model
    pub provider: String,
    pub model: String,
    pub api_base: String,
    pub api_key_masked: String,
    pub api_key_set: bool,
    // General
    pub user_name: String,
    pub data_dir: String,
    pub log_level: String,
    // Tools
    pub tools_enabled: bool,
    // Memory
    pub memory_max_entries: i64,
    pub memory_fts_enabled: bool,
    pub memory_db_path: String,
    // Skills
    pub skills_dirs: Vec<String>,
    pub skills_auto_create: bool,
    // Channels (masked; only "set" booleans + masked previews leave the backend)
    pub discord_token_masked: String,
    pub discord_token_set: bool,
    pub telegram_token_masked: String,
    pub telegram_token_set: bool,
    pub slack_token_masked: String,
    pub slack_token_set: bool,
    // Gateway server + Discord access
    pub webhook_host: String,
    pub webhook_port: u16,
    pub discord_allowed_users: Vec<String>,
    pub dm_policy: String,
    // Security
    pub dm_pairing: bool,
    // Vision
    pub vision_provider: String,
    pub vision_gemini_path: String,
    // App meta
    pub app_version: String,
}

/// Full editable config from the expanded Settings view. Secret fields are
/// `Option<String>` and only overwrite when `Some` and non-empty, so re-saving
/// from a masked field never wipes an existing secret.
#[derive(Debug, Deserialize)]
pub struct FullConfigDto {
    pub provider: String,
    pub model: String,
    pub api_base: String,
    pub api_key: Option<String>,
    pub user_name: String,
    pub log_level: String,
    pub tools_enabled: bool,
    pub memory_max_entries: i64,
    pub memory_fts_enabled: bool,
    pub skills_dirs: Vec<String>,
    pub skills_auto_create: bool,
    pub discord_token: Option<String>,
    pub telegram_token: Option<String>,
    pub slack_token: Option<String>,
    #[serde(default)]
    pub webhook_host: String,
    #[serde(default)]
    pub webhook_port: u16,
    #[serde(default)]
    pub discord_allowed_users: Vec<String>,
    #[serde(default)]
    pub dm_policy: String,
    pub dm_pairing: bool,
    pub vision_provider: String,
    pub vision_gemini_path: String,
}

fn set_if_present(field: &mut String, incoming: Option<String>) {
    if let Some(v) = incoming {
        let v = v.trim();
        if !v.is_empty() {
            *field = v.to_string();
        }
    }
}

/// Read the full config for Settings. The api_key and channel tokens are masked.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_config() -> Result<ConfigDto, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    Ok(ConfigDto {
        provider: cfg.model.provider,
        model: cfg.model.model,
        api_base: cfg.model.api_base,
        api_key_masked: mask_key(&cfg.model.api_key),
        api_key_set: !cfg.model.api_key.trim().is_empty(),
        user_name: cfg.general.user_name,
        data_dir: cfg.general.data_dir,
        log_level: cfg.general.log_level,
        tools_enabled: cfg.tools.enabled,
        memory_max_entries: cfg.memory.max_entries as i64,
        memory_fts_enabled: cfg.memory.fts_enabled,
        memory_db_path: cfg.memory.db_path,
        skills_dirs: cfg.skills.dirs,
        skills_auto_create: cfg.skills.auto_create,
        discord_token_masked: mask_key(&cfg.gateway.discord_token),
        discord_token_set: !cfg.gateway.discord_token.trim().is_empty(),
        telegram_token_masked: mask_key(&cfg.gateway.telegram_token),
        telegram_token_set: !cfg.gateway.telegram_token.trim().is_empty(),
        slack_token_masked: mask_key(&cfg.gateway.slack_token),
        slack_token_set: !cfg.gateway.slack_token.trim().is_empty(),
        webhook_host: cfg.gateway.webhook_host,
        webhook_port: cfg.gateway.webhook_port,
        discord_allowed_users: cfg.gateway.discord_allowed_users,
        dm_policy: cfg.gateway.dm_policy,
        dm_pairing: cfg.security.dm_pairing,
        vision_provider: cfg.vision.provider,
        vision_gemini_path: cfg.vision.gemini_path,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// A named API provider (from `[[providers]]`), secret masked.
#[derive(Debug, Serialize)]
pub struct ProviderEntryDto {
    pub name: String,
    pub api_base: String,
    pub api_key_masked: String,
    pub api_key_set: bool,
}

/// One configured per-modality route (from `[routing]`).
#[derive(Debug, Serialize)]
pub struct ModalityRouteDto {
    pub modality: String,
    pub provider: String,
    pub model: String,
}

/// Provider/routing view: the active default provider plus any named providers
/// and per-modality routes used for multi-model routing. Read-only.
#[derive(Debug, Serialize)]
pub struct ProvidersDto {
    pub active_provider: String,
    pub active_model: String,
    pub active_api_base: String,
    pub providers: Vec<ProviderEntryDto>,
    pub routing: Vec<ModalityRouteDto>,
}

/// Read the configured providers + routing for the Providers view. Secrets masked.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_providers() -> Result<ProvidersDto, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let providers = cfg
        .providers
        .iter()
        .map(|p| ProviderEntryDto {
            name: p.name.clone(),
            api_base: p.api_base.clone(),
            api_key_masked: mask_key(&p.api_key),
            api_key_set: !p.api_key.trim().is_empty(),
        })
        .collect();
    let routing = [
        ("text", &cfg.routing.text),
        ("vision", &cfg.routing.vision),
        ("image_gen", &cfg.routing.image_gen),
        ("video", &cfg.routing.video),
    ]
    .into_iter()
    .filter(|(_, r)| !r.provider.trim().is_empty() || !r.model.trim().is_empty())
    .map(|(m, r)| ModalityRouteDto {
        modality: m.to_string(),
        provider: r.provider.clone(),
        model: r.model.clone(),
    })
    .collect();
    Ok(ProvidersDto {
        active_provider: cfg.model.provider,
        active_model: cfg.model.model,
        active_api_base: cfg.model.api_base,
        providers,
        routing,
    })
}

/// Persist the Model section only. Kept as a stable, minimal command (used by the
/// existing E2E tests and the Settings Model section). Bypasses `config::set()`.
#[tauri::command(rename_all = "snake_case")]
pub async fn save_config(
    state: State<'_, AppCore>,
    model: String,
    api_base: String,
    api_key: Option<String>,
) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    let mut cfg = config::load().await.map_err(|e| e.to_string())?;
    cfg.model.model = model.trim().to_string();
    cfg.model.api_base = api_base.trim().to_string();
    set_if_present(&mut cfg.model.api_key, api_key);
    config::save(&cfg).await.map_err(|e| e.to_string())?;
    rebuild_agent_if_model_changed(&mut turn, &cfg);
    Ok(())
}

/// Persist the full config from the expanded Settings view.
#[tauri::command(rename_all = "snake_case")]
pub async fn save_full_config(state: State<'_, AppCore>, dto: FullConfigDto) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    let mut cfg = config::load().await.map_err(|e| e.to_string())?;

    cfg.model.provider = dto.provider.trim().to_string();
    cfg.model.model = dto.model.trim().to_string();
    cfg.model.api_base = dto.api_base.trim().to_string();
    set_if_present(&mut cfg.model.api_key, dto.api_key);

    cfg.general.user_name = dto.user_name.trim().to_string();
    cfg.general.log_level = dto.log_level.trim().to_string();

    cfg.tools.enabled = dto.tools_enabled;

    cfg.memory.max_entries = dto.memory_max_entries.max(0) as usize;
    cfg.memory.fts_enabled = dto.memory_fts_enabled;

    cfg.skills.dirs = dto.skills_dirs.into_iter().filter(|d| !d.trim().is_empty()).collect();
    cfg.skills.auto_create = dto.skills_auto_create;

    set_if_present(&mut cfg.gateway.discord_token, dto.discord_token);
    set_if_present(&mut cfg.gateway.telegram_token, dto.telegram_token);
    set_if_present(&mut cfg.gateway.slack_token, dto.slack_token);

    cfg.gateway.webhook_host = dto.webhook_host.trim().to_string();
    cfg.gateway.webhook_port = dto.webhook_port;
    cfg.gateway.discord_allowed_users = dto
        .discord_allowed_users
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    cfg.gateway.dm_policy = dto.dm_policy.trim().to_string();

    cfg.security.dm_pairing = dto.dm_pairing;
    cfg.vision.provider = dto.vision_provider.trim().to_string();
    cfg.vision.gemini_path = dto.vision_gemini_path.trim().to_string();

    config::save(&cfg).await.map_err(|e| e.to_string())?;

    turn.agent.tools_enabled = cfg.tools.enabled;
    rebuild_agent_if_model_changed(&mut turn, &cfg);
    Ok(())
}

/// Toggle tool execution and persist it so the choice survives a restart.
#[tauri::command(rename_all = "snake_case")]
pub async fn set_tools_enabled(state: State<'_, AppCore>, enabled: bool) -> Result<(), String> {
    let mut turn = state.turn.lock().await;
    turn.agent.tools_enabled = enabled;
    let mut cfg = config::load().await.map_err(|e| e.to_string())?;
    cfg.tools.enabled = enabled;
    config::save(&cfg).await.map_err(|e| e.to_string())?;
    Ok(())
}

fn rebuild_agent_if_model_changed(turn: &mut crate::state::Turn, cfg: &config::Config) {
    if turn.agent.model != cfg.model.model {
        let tools = turn.agent.tools_enabled;
        let ws = turn.agent.workspace_dir.clone();
        turn.agent = Agent::new(cfg.model.model.clone())
            .with_workspace(ws)
            .with_tools_enabled(tools);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_if_present_only_overwrites_with_nonempty() {
        let mut field = "keep-me".to_string();
        set_if_present(&mut field, None);
        assert_eq!(field, "keep-me", "None must not wipe an existing secret");
        set_if_present(&mut field, Some("   ".into()));
        assert_eq!(field, "keep-me", "blank must not wipe an existing secret");
        set_if_present(&mut field, Some("new-value".into()));
        assert_eq!(field, "new-value");
    }

    #[test]
    fn full_config_dto_deserializes_from_frontend_payload() {
        let json = serde_json::json!({
            "provider": "openrouter", "model": "m", "api_base": "https://x/v1",
            "api_key": null, "user_name": "u", "log_level": "info",
            "tools_enabled": true, "memory_max_entries": 50000, "memory_fts_enabled": true,
            "skills_dirs": ["a", "b"], "skills_auto_create": false,
            "discord_token": null, "telegram_token": null, "slack_token": null,
            "dm_pairing": true, "vision_provider": "gemini-cli", "vision_gemini_path": "gemini"
        });
        let dto: FullConfigDto = serde_json::from_value(json).expect("deserialize");
        assert!(dto.api_key.is_none());
        assert!(dto.tools_enabled);
        assert_eq!(dto.skills_dirs, vec!["a", "b"]);
        assert_eq!(dto.memory_max_entries, 50000);
    }
}
