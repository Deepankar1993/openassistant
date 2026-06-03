// src/config/mod.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub general: GeneralConfig,
    pub model: ModelConfig,
    pub gateway: GatewayConfig,
    pub memory: MemoryConfig,
    pub skills: SkillsConfig,
    pub security: SecurityConfig,
    pub vision: VisionConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    /// Named API providers for multi-model routing. Empty by default; legacy
    /// configs (which lack this key) deserialize to an empty list.
    #[serde(default)]
    pub providers: Vec<ProviderEntry>,
    /// Per-modality model routing. Empty routes fall through to `model` (below),
    /// so an absent/empty `routing` block reproduces single-model behavior.
    #[serde(default)]
    pub routing: RoutingConfig,
}

/// A named, OpenAI-compatible API provider (base URL + key).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProviderEntry {
    pub name: String,
    pub api_base: String,
    pub api_key: String,
}

/// A single modality's route: which provider + which model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModalityRoute {
    pub provider: String,
    pub model: String,
}

/// Routing map across modalities. `text` is dispatched today; `vision`,
/// `image_gen`, and `video` are parsed and stored but not yet dispatched.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RoutingConfig {
    pub text: ModalityRoute,
    pub vision: ModalityRoute,
    pub image_gen: ModalityRoute,
    pub video: ModalityRoute,
}

/// Tool-execution posture. The desktop app constructs the agent from this so a
/// user's opt-in (or opt-out) of shell/file access survives restarts.
/// `#[serde(default)]` on the `Config.tools` field keeps existing config.yaml
/// files (written before this section existed) loadable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub enabled: bool,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub data_dir: String,
    pub log_level: String,
    pub name: String,
    pub user_name: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            log_level: "info".to_string(),
            name: "openAssistant".to_string(),
            user_name: "User".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub api_base: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: "openrouter".to_string(),
            model: "openrouter/owl-alpha".to_string(),
            api_key: String::new(),
            api_base: "https://openrouter.ai/api/v1".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayConfig {
    pub discord_token: String,
    pub discord_allowed_users: Vec<String>,
    pub telegram_token: String,
    pub slack_token: String,
    pub slack_signing_secret: String,
    pub webhook_port: u16,
    pub dm_policy: String, // "pairing" or "open"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub db_path: String,
    pub max_entries: usize,
    pub fts_enabled: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            db_path: format!("{}/memory.db", default_data_dir()),
            max_entries: 100_000,
            fts_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    pub dirs: Vec<String>,
    pub auto_create: bool,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            dirs: vec![format!("{}/skills", default_data_dir())],
            auto_create: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub dm_pairing: bool,
    pub allow_from: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            dm_pairing: true,
            allow_from: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionConfig {
    pub provider: String,
    pub gemini_path: String,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            provider: "gemini-cli".to_string(),
            gemini_path: "gemini".to_string(),
        }
    }
}

fn default_data_dir() -> String {
    format!("{}/.openassistant", std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string()))
}

pub async fn load() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        let content = tokio::fs::read_to_string(&path).await?;
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    } else {
        let config = Config::default();
        save(&config).await?;
        Ok(config)
    }
}

pub async fn save(config: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = serde_yaml::to_string(config)?;
    tokio::fs::write(&path, content).await?;
    Ok(())
}

pub async fn set(key: &str, value: &str) -> Result<()> {
    let mut config = load().await?;
    match key {
        "model.provider" => config.model.provider = value.to_string(),
        "model.model" => config.model.model = value.to_string(),
        "model.api_key" => config.model.api_key = value.to_string(),
        "gateway.discord_token" => config.gateway.discord_token = value.to_string(),
        "gateway.discord_allowed_users" => {
            config.gateway.discord_allowed_users = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
        "gateway.dm_policy" => config.gateway.dm_policy = value.to_string(),
        "gateway.telegram_token" => config.gateway.telegram_token = value.to_string(),
        "gateway.slack_token" => config.gateway.slack_token = value.to_string(),
        "security.dm_pairing" => config.security.dm_pairing = value.parse().unwrap_or(true),
        _ => tracing::warn!("Unknown config key: {}", key),
    }
    save(&config).await?;
    Ok(())
}

pub async fn show_all() -> Result<()> {
    let config = load().await?;
    println!("{}", serde_yaml::to_string(&config).unwrap_or_default());
    Ok(())
}

pub async fn check() -> Result<()> {
    let _ = load().await?;
    Ok(())
}

/// Resolve the `(api_base, api_key, model)` to use for a given modality
/// (`"text"`, `"vision"`, `"image_gen"`, `"video"`).
///
/// Routing is strictly opt-in: a route is honored only when its `provider`
/// matches an entry in `providers[]` AND its `model` is set. Otherwise this
/// falls through to the legacy `model` block, so an empty `routing` reproduces
/// the single-model behavior byte-for-byte.
pub fn resolve_provider<'a>(config: &'a Config, modality: &str) -> (&'a str, &'a str, &'a str) {
    let route = match modality {
        "vision" => &config.routing.vision,
        "image_gen" => &config.routing.image_gen,
        "video" => &config.routing.video,
        _ => &config.routing.text,
    };
    if !route.provider.is_empty() && !route.model.is_empty() {
        if let Some(p) = config.providers.iter().find(|p| p.name == route.provider) {
            return (&p.api_base, &p.api_key, &route.model);
        }
    }
    (&config.model.api_base, &config.model.api_key, &config.model.model)
}

fn config_path() -> PathBuf {
    PathBuf::from(format!("{}/config.yaml", data_dir_default()))
}

/// Public accessor for default data directory
pub fn data_dir_default() -> String {
    default_data_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_yaml_round_trip_preserves_model_fields() {
        let mut cfg = Config::default();
        cfg.model.model = "anthropic/claude-opus".to_string();
        cfg.model.api_base = "https://example.test/v1".to_string();
        cfg.model.api_key = "secret-123".to_string();

        let yaml = serde_yaml::to_string(&cfg).expect("serialize");
        let back: Config = serde_yaml::from_str(&yaml).expect("deserialize");

        assert_eq!(back.model.model, "anthropic/claude-opus");
        assert_eq!(back.model.api_base, "https://example.test/v1");
        assert_eq!(back.model.api_key, "secret-123");
        assert_eq!(back.model.provider, cfg.model.provider);
    }

    #[test]
    fn defaults_are_sane() {
        let cfg = Config::default();
        assert_eq!(cfg.model.provider, "openrouter");
        assert!(cfg.model.api_key.is_empty(), "ships with no api key");
        assert!(cfg.model.api_base.starts_with("https://"));
    }

    #[test]
    fn legacy_config_without_routing_loads_and_falls_through() {
        // A config.yaml written before multi-model routing existed: no
        // `providers` / `routing` keys at all.
        let legacy = "
general:
  data_dir: /tmp/oa
  log_level: info
  name: openAssistant
  user_name: User
model:
  provider: openrouter
  model: openrouter/owl-alpha
  api_key: legacy-key
  api_base: https://openrouter.ai/api/v1
gateway:
  discord_token: ''
  discord_allowed_users: []
  telegram_token: ''
  slack_token: ''
  slack_signing_secret: ''
  webhook_port: 0
  dm_policy: pairing
memory:
  db_path: /tmp/oa/memory.db
  max_entries: 100000
  fts_enabled: true
skills:
  dirs: []
  auto_create: true
security:
  dm_pairing: true
  allow_from: []
vision:
  provider: gemini-cli
  gemini_path: gemini
";
        let cfg: Config = serde_yaml::from_str(legacy).expect("legacy config must still load");
        assert!(cfg.providers.is_empty());
        // Empty routing => resolve_provider returns the legacy model block.
        let (base, key, model) = resolve_provider(&cfg, "text");
        assert_eq!(base, "https://openrouter.ai/api/v1");
        assert_eq!(key, "legacy-key");
        assert_eq!(model, "openrouter/owl-alpha");
    }

    #[test]
    fn populated_routing_selects_provider() {
        let mut cfg = Config::default();
        cfg.providers.push(ProviderEntry {
            name: "openai".into(),
            api_base: "https://api.openai.com/v1".into(),
            api_key: "sk-openai".into(),
        });
        cfg.routing.text = ModalityRoute { provider: "openai".into(), model: "gpt-4o".into() };

        let (base, key, model) = resolve_provider(&cfg, "text");
        assert_eq!(base, "https://api.openai.com/v1");
        assert_eq!(key, "sk-openai");
        assert_eq!(model, "gpt-4o");

        // An unconfigured modality still falls through to the legacy block.
        let (_, _, vmodel) = resolve_provider(&cfg, "vision");
        assert_eq!(vmodel, cfg.model.model);
    }
}
