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
    /// Bridge to a locally-installed Claude Code CLI (`claude`). When enabled,
    /// openAssistant can delegate turns to `claude -p` with session continuity.
    #[serde(default)]
    pub claude: ClaudeBridgeConfig,
    /// Built-in tool permission posture. Rules apply at every mode (deny beats
    /// bypass); `gateway_mode` caps remote channels (Discord/Telegram/Slack/
    /// WebChat) the same way the claude bridge caps remote origins.
    #[serde(default)]
    pub permissions: PermissionsConfig,
    /// Proactive daily brief: the assistant messages you first each morning
    /// (Discord home channel and/or a Telegram chat) and posts URL-watcher
    /// change notifications. The `brief` CLI command works regardless.
    #[serde(default)]
    pub brief: BriefConfig,
}

/// Daily-brief / proactive messaging settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BriefConfig {
    /// Master switch for scheduled delivery (the gateway's proactive loop).
    pub enabled: bool,
    /// Local time of day to deliver, "HH:MM" 24h. Invalid values fall back to 08:00.
    pub time: String,
    /// Post to the Discord home channel (gateway.discord_home_channel) when set.
    pub discord: bool,
    /// Telegram chat id to post to; empty ⇒ skip Telegram.
    pub telegram_chat_id: String,
}

impl Default for BriefConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            time: "08:00".to_string(),
            discord: true,
            telegram_chat_id: String::new(),
        }
    }
}

/// Configuration for the Claude Code CLI bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeBridgeConfig {
    /// Master switch for the bridge (the `claude` tool + Discord routing).
    pub enabled: bool,
    /// Path/name of the claude binary.
    pub bin: String,
    /// Working directory claude runs in (the project to operate on). Empty ⇒
    /// the current directory, falling back to the data dir.
    pub workspace: String,
    /// Optional model alias/name (e.g. "opus", "sonnet"); empty ⇒ claude default.
    pub model: String,
    /// Permission mode: default | acceptEdits | plan | bypassPermissions | dontAsk | auto.
    pub permission_mode: String,
    /// If true, pass `--dangerously-skip-permissions` (full autonomy — use with care).
    pub skip_permissions: bool,
    /// Extra text appended to claude's system prompt (persona/tone).
    pub append_system_prompt: String,
    /// Per-call timeout in seconds.
    pub timeout_secs: u64,
    /// When the bridge is enabled, route Discord conversations through it by default.
    pub discord_default: bool,
}

impl Default for ClaudeBridgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bin: "claude".to_string(),
            workspace: String::new(),
            model: String::new(),
            permission_mode: "acceptEdits".to_string(),
            skip_permissions: false,
            append_system_prompt: String::new(),
            timeout_secs: 300,
            discord_default: true,
        }
    }
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
/// Permission posture for the built-in agent's tools. Local front-ends keep
/// full autonomy (BypassPermissions) to preserve pre-existing behavior; the
/// gateway constructs agents with `gateway_mode` instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionsConfig {
    /// Permission mode for remote gateway channels: default | acceptEdits |
    /// auto | bypassPermissions.
    pub gateway_mode: String,
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            gateway_mode: "acceptEdits".to_string(),
            allow: vec![],
            ask: vec![],
            deny: vec![],
        }
    }
}

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

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub discord_token: String,
    #[serde(default)]
    pub discord_allowed_users: Vec<String>,
    /// Optional "home" channel id: top-level messages here auto-spawn a thread
    /// (Hermes-style), even without an @mention. Set via the `set home` command.
    #[serde(default)]
    pub discord_home_channel: String,
    /// Hours between automatic "self-improvement review" posts to the home
    /// channel (0 = disabled).
    #[serde(default)]
    pub discord_review_hours: u64,
    /// Status-reaction lifecycle on inbound messages: 👀 while working →
    /// ✅ on success / ❌ on error (Hermes-style). Set false to disable.
    #[serde(default = "default_true")]
    pub discord_reactions: bool,
    /// When true (default), the bot only answers guild messages that @mention
    /// it. When false, it answers every message in a guild channel.
    #[serde(default = "default_true")]
    pub discord_require_mention: bool,
    /// Channel ids where the bot answers WITHOUT an @mention and replies inline
    /// (no auto-thread) — lightweight free-response chat rooms.
    #[serde(default)]
    pub discord_free_response_channels: Vec<String>,
    /// In shared (free-response) channels, isolate conversation history per user
    /// (Hermes default) instead of one shared room-wide conversation.
    #[serde(default = "default_true")]
    pub discord_group_sessions_per_user: bool,
    pub telegram_token: String,
    pub slack_token: String,
    pub slack_signing_secret: String,
    /// Bind address for the WebChat/Slack server. Empty ⇒ "0.0.0.0".
    /// `#[serde(default)]` keeps configs written before this field existed loadable.
    #[serde(default)]
    pub webhook_host: String,
    pub webhook_port: u16,
    pub dm_policy: String, // "pairing" or "open"
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            discord_token: String::new(),
            discord_allowed_users: Vec::new(),
            discord_home_channel: String::new(),
            discord_review_hours: 0,
            discord_reactions: true,
            discord_require_mention: true,
            discord_free_response_channels: Vec::new(),
            discord_group_sessions_per_user: true,
            telegram_token: String::new(),
            slack_token: String::new(),
            slack_signing_secret: String::new(),
            webhook_host: String::new(),
            webhook_port: 0,
            dm_policy: String::new(),
        }
    }
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

/// Parse a comma-separated config value into a trimmed, non-empty list.
fn split_list(value: &str) -> Vec<String> {
    value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
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
            config.gateway.discord_allowed_users = split_list(value)
        }
        "gateway.dm_policy" => config.gateway.dm_policy = value.to_string(),
        "gateway.discord_home_channel" => config.gateway.discord_home_channel = value.to_string(),
        "gateway.discord_review_hours" => {
            config.gateway.discord_review_hours = value
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid hours '{}' (expected a non-negative integer)", value))?
        }
        "gateway.discord_reactions" => {
            config.gateway.discord_reactions = value.parse().unwrap_or(true)
        }
        "gateway.discord_require_mention" => {
            config.gateway.discord_require_mention = value.parse().unwrap_or(true)
        }
        "gateway.discord_free_response_channels" => {
            config.gateway.discord_free_response_channels = split_list(value)
        }
        "gateway.discord_group_sessions_per_user" => {
            config.gateway.discord_group_sessions_per_user = value.parse().unwrap_or(true)
        }
        "gateway.webhook_host" => config.gateway.webhook_host = value.to_string(),
        "gateway.webhook_port" => {
            config.gateway.webhook_port = value
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid port '{}' (expected 0-65535)", value))?
        }
        "gateway.telegram_token" => config.gateway.telegram_token = value.to_string(),
        "gateway.slack_token" => config.gateway.slack_token = value.to_string(),
        "security.dm_pairing" => config.security.dm_pairing = value.parse().unwrap_or(true),
        "claude.enabled" => config.claude.enabled = value.parse().unwrap_or(false),
        "claude.bin" => config.claude.bin = value.to_string(),
        "claude.workspace" => config.claude.workspace = value.to_string(),
        "claude.model" => config.claude.model = value.to_string(),
        "claude.permission_mode" => config.claude.permission_mode = value.to_string(),
        "claude.skip_permissions" => config.claude.skip_permissions = value.parse().unwrap_or(false),
        "claude.append_system_prompt" => config.claude.append_system_prompt = value.to_string(),
        "claude.discord_default" => config.claude.discord_default = value.parse().unwrap_or(true),
        "claude.timeout_secs" => {
            config.claude.timeout_secs = value
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid timeout '{}' (expected seconds)", value))?
        }
        "brief.enabled" => config.brief.enabled = value.parse().unwrap_or(false),
        "brief.time" => config.brief.time = value.to_string(),
        "brief.discord" => config.brief.discord = value.parse().unwrap_or(true),
        "brief.telegram_chat_id" => config.brief.telegram_chat_id = value.to_string(),
        "permissions.gateway_mode" => config.permissions.gateway_mode = value.to_string(),
        "permissions.allow" => config.permissions.allow = split_list(value),
        "permissions.ask" => config.permissions.ask = split_list(value),
        "permissions.deny" => config.permissions.deny = split_list(value),
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

/// Resolve the WebChat/Slack bind host. Empty config ⇒ `0.0.0.0`.
pub fn webchat_host(config: &Config) -> String {
    let h = config.gateway.webhook_host.trim();
    if h.is_empty() { "0.0.0.0".to_string() } else { h.to_string() }
}

/// Resolve the WebChat/Slack port. `0` (unset) ⇒ `3000`.
pub fn webchat_port(config: &Config) -> u16 {
    if config.gateway.webhook_port == 0 { 3000 } else { config.gateway.webhook_port }
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
    fn permissions_config_defaults_and_round_trip() {
        let cfg = Config::default();
        assert_eq!(cfg.permissions.gateway_mode, "acceptEdits");
        assert!(cfg.permissions.deny.is_empty());

        let mut cfg2 = Config::default();
        cfg2.permissions.deny = vec!["Bash(rm *)".into()];
        cfg2.permissions.gateway_mode = "default".into();
        let yaml = serde_yaml::to_string(&cfg2).expect("serialize");
        let back: Config = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(back.permissions.deny, vec!["Bash(rm *)".to_string()]);
        assert_eq!(back.permissions.gateway_mode, "default");

        // YAML written before the `permissions:` key existed must still load
        // (serde default). Same shape as the legacy-routing test fixture.
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
        let cfg3: Config = serde_yaml::from_str(legacy).expect("legacy loads");
        assert_eq!(cfg3.permissions.gateway_mode, "acceptEdits");
    }

    #[test]
    fn brief_config_defaults_and_round_trip() {
        let cfg = Config::default();
        assert!(!cfg.brief.enabled);
        assert_eq!(cfg.brief.time, "08:00");
        assert!(cfg.brief.discord);
        assert!(cfg.brief.telegram_chat_id.is_empty());

        let mut cfg2 = Config::default();
        cfg2.brief.enabled = true;
        cfg2.brief.time = "07:30".into();
        cfg2.brief.telegram_chat_id = "12345".into();
        let yaml = serde_yaml::to_string(&cfg2).expect("serialize");
        let back: Config = serde_yaml::from_str(&yaml).expect("deserialize");
        assert!(back.brief.enabled);
        assert_eq!(back.brief.time, "07:30");
        assert_eq!(back.brief.telegram_chat_id, "12345");

        // Legacy YAML without a `brief:` key still loads (serde default).
        let legacy = "general:\n  data_dir: /tmp/oa\nmodel:\n  provider: openrouter\n  model: m\n  api_key: ''\n  api_base: https://x\ngateway:\n  discord_token: ''\n  discord_allowed_users: []\n  telegram_token: ''\n  slack_token: ''\n  slack_signing_secret: ''\n  webhook_port: 0\n  dm_policy: open\nmemory:\n  db_path: /tmp/oa/m.db\n  max_entries: 1\n  fts_enabled: false\nskills:\n  dirs: []\n  auto_create: false\nsecurity:\n  dm_pairing: false\n  allow_from: []\nvision:\n  provider: g\n  gemini_path: g\n";
        let cfg3: Config = serde_yaml::from_str(legacy).expect("legacy loads");
        assert!(!cfg3.brief.enabled);
    }

    #[test]
    fn gateway_discord_interaction_defaults_and_round_trip() {
        // New Hermes-parity toggles default ON (manual Default, not derived).
        let cfg = Config::default();
        assert!(cfg.gateway.discord_reactions);
        assert!(cfg.gateway.discord_require_mention);
        assert!(cfg.gateway.discord_group_sessions_per_user);
        assert!(cfg.gateway.discord_free_response_channels.is_empty());

        let mut cfg2 = Config::default();
        cfg2.gateway.discord_reactions = false;
        cfg2.gateway.discord_require_mention = false;
        cfg2.gateway.discord_free_response_channels = vec!["123".into(), "456".into()];
        cfg2.gateway.discord_group_sessions_per_user = false;
        let yaml = serde_yaml::to_string(&cfg2).expect("serialize");
        let back: Config = serde_yaml::from_str(&yaml).expect("deserialize");
        assert!(!back.gateway.discord_reactions);
        assert!(!back.gateway.discord_require_mention);
        assert!(!back.gateway.discord_group_sessions_per_user);
        assert_eq!(back.gateway.discord_free_response_channels, vec!["123", "456"]);

        // Legacy gateway block (no new keys) loads with the toggles defaulting ON.
        let legacy = "general:\n  data_dir: /tmp/oa\nmodel:\n  provider: openrouter\n  model: m\n  api_key: ''\n  api_base: https://x\ngateway:\n  discord_token: ''\n  discord_allowed_users: []\n  telegram_token: ''\n  slack_token: ''\n  slack_signing_secret: ''\n  webhook_port: 0\n  dm_policy: open\nmemory:\n  db_path: /tmp/oa/m.db\n  max_entries: 1\n  fts_enabled: false\nskills:\n  dirs: []\n  auto_create: false\nsecurity:\n  dm_pairing: false\n  allow_from: []\nvision:\n  provider: g\n  gemini_path: g\n";
        let cfg3: Config = serde_yaml::from_str(legacy).expect("legacy loads");
        assert!(cfg3.gateway.discord_reactions);
        assert!(cfg3.gateway.discord_require_mention);
        assert!(cfg3.gateway.discord_group_sessions_per_user);
        assert!(cfg3.gateway.discord_free_response_channels.is_empty());
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
