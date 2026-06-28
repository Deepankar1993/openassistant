// src/gateway/mod.rs
pub mod attachments;
pub mod discord;
pub mod discord_store;
pub mod proactive;
pub mod session_store;
pub mod telegram;
pub mod slack;
pub mod webchat;

use anyhow::Result;
use serde::Serialize;
use tracing::{info, warn};

use crate::config::Config;
use crate::security::allowlist::is_allowed;

/// Shared access-control gate for messaging channels, mirroring Discord's policy
/// (`discord.rs::gate`). Returns true if `user_id` may be served.
///
/// - Non-empty `allowed`: only ids in the list (or the `"*"` wildcard) pass.
/// - Empty `allowed`: open **iff** `dm_policy == "open"`; otherwise everyone is
///   ignored (fail-closed). Reuses `security::allowlist::is_allowed`.
pub fn gate(user_id: &str, allowed: &[String], dm_policy: &str) -> bool {
    if !allowed.is_empty() {
        return is_allowed(user_id, allowed);
    }
    dm_policy == "open"
}

/// One gateway setup requirement, surfaced identically in the CLI
/// (`openassistant gateway --check`) and the desktop Channels panel.
#[derive(Debug, Clone, Serialize)]
pub struct GatewayRequirement {
    /// Short label, e.g. "API key" or "Discord".
    pub name: String,
    /// Whether this item is satisfied / ready.
    pub ok: bool,
    /// Required for any channel to function (vs. an optional channel).
    pub required: bool,
    /// Human-readable status plus an actionable hint.
    pub detail: String,
}

fn req(name: &str, ok: bool, required: bool, detail: impl Into<String>) -> GatewayRequirement {
    GatewayRequirement { name: name.into(), ok, required, detail: detail.into() }
}

/// Build the gateway readiness report from config. Pure (no I/O) so both the
/// CLI and the desktop can call it.
pub fn readiness(config: &Config) -> Vec<GatewayRequirement> {
    let mut out = Vec::new();
    let g = &config.gateway;

    // Required: an LLM key, or no channel can actually answer.
    let key_set = !config.model.api_key.trim().is_empty();
    out.push(req(
        "API key",
        key_set,
        true,
        if key_set {
            format!("Model API key is set (provider: {}).", config.model.provider)
        } else {
            "No API key — set it in the desktop Settings → Model, or run \
             `openassistant config --key model.api_key --value <KEY>`."
                .to_string()
        },
    ));

    // WebChat is always available once the server runs.
    let host = crate::config::webchat_host(config);
    let port = crate::config::webchat_port(config);
    out.push(req(
        "WebChat server",
        true,
        false,
        format!("Will listen on http://{host}:{port} (set gateway.webhook_host / gateway.webhook_port to change)."),
    ));

    // Discord.
    let discord_set = !g.discord_token.trim().is_empty();
    let gate_open = !g.discord_allowed_users.is_empty() || g.dm_policy == "open";
    out.push(if !discord_set {
        req("Discord", false, false, "Not configured (optional). Set gateway.discord_token to enable.")
    } else if !gate_open {
        req("Discord", false, false,
            "Token set, but no allowlist and dm_policy isn't 'open' — the bot will ignore everyone. \
             Set gateway.discord_allowed_users (your numeric user ID) or gateway.dm_policy=open.")
    } else {
        req("Discord", true, false,
            "Ready. Reminder: enable the MESSAGE CONTENT intent in the Discord Developer Portal.")
    });

    // Telegram.
    let telegram_set = !g.telegram_token.trim().is_empty();
    out.push(if telegram_set {
        req("Telegram", true, false, "Ready (Bot API long polling).")
    } else {
        req("Telegram", false, false, "Not configured (optional). Set gateway.telegram_token to enable.")
    });

    // Slack.
    let slack_token = !g.slack_token.trim().is_empty();
    let slack_secret = !g.slack_signing_secret.trim().is_empty();
    out.push(if slack_token && slack_secret {
        req("Slack", true, false,
            format!("Ready. Point your Slack app's Events URL at http://<public-host>:{port}/slack/events (needs a public URL)."))
    } else if slack_token || slack_secret {
        req("Slack", false, false, "Partially configured — needs BOTH gateway.slack_token and gateway.slack_signing_secret.")
    } else {
        req("Slack", false, false, "Not configured (optional).")
    });

    // How to run — surfaced on both CLI and desktop, including the PATH fallback.
    out.push(req(
        "How to run",
        true,
        false,
        "Start the gateway with `openassistant gateway`. If `openassistant` isn't on your PATH, \
         run `cargo run -- gateway` from the project, or the built binary at \
         `target/release/openassistant` (or `target/debug/openassistant`).",
    ));

    out
}

/// Format a readiness report for terminal output.
pub fn format_readiness(reqs: &[GatewayRequirement]) -> String {
    let mut s = String::from("Gateway readiness\n─────────────────\n");
    for r in reqs {
        let icon = if r.ok { "✅" } else if r.required { "❌" } else { "⚠️ " };
        s.push_str(&format!("{icon} {}: {}\n", r.name, r.detail));
    }
    let unmet: Vec<&str> = reqs.iter().filter(|r| r.required && !r.ok).map(|r| r.name.as_str()).collect();
    if unmet.is_empty() {
        s.push_str("\nAll required items satisfied — the gateway can run.\n");
    } else {
        s.push_str(&format!("\nMissing required: {}.\n", unmet.join(", ")));
    }
    s
}

/// Spawn the optional channels (Discord/Telegram) and run the WebChat server in
/// the foreground (blocks until it stops). Shared by the CLI (`start_gateway`)
/// and the desktop (`start_gateway_handle`).
async fn run_all(config: Config) -> Result<()> {
    // One MCP registry (owns the server subprocesses) shared across channels.
    let mcp = build_mcp(&config).await;

    // Discord/Telegram run on their own tasks; failures are logged, not lost.
    if !config.gateway.discord_token.is_empty() {
        info!("Discord token configured, starting Discord handler...");
        let cfg = config.clone();
        let mcp = mcp.clone();
        tokio::spawn(async move {
            if let Err(e) = discord::start(cfg, mcp).await {
                tracing::error!("Discord gateway error: {}", e);
            }
        });
    }
    if !config.gateway.telegram_token.is_empty() {
        info!("Telegram token configured, starting Telegram handler...");
        let cfg = config.clone();
        let mcp = mcp.clone();
        tokio::spawn(async move {
            if let Err(e) = telegram::start(cfg, mcp).await {
                tracing::error!("Telegram gateway error: {}", e);
            }
        });
    }
    if !config.gateway.slack_token.is_empty() || !config.gateway.slack_signing_secret.is_empty() {
        info!("Slack configured — Events endpoint will be served at POST /slack/events (requires a public URL).");
    }

    // Proactive loop (daily brief + watchers): cheap 60s tick that re-reads
    // config, so brief.enabled / watcher edits apply without a restart.
    {
        let cfg = config.clone();
        tokio::spawn(proactive::proactive_loop(cfg));
    }

    info!("Starting WebChat messaging server (real agent loop)...");
    webchat::start(config, mcp).await
}

/// Build the shared MCP registry from `<data_dir>/.mcp.json` and initialize its
/// servers. Returns None when nothing is configured / on error (logged).
async fn build_mcp(config: &Config) -> Option<std::sync::Arc<crate::core::mcp::McpRegistry>> {
    match crate::core::mcp::McpRegistry::open_default(&config.general.data_dir) {
        Ok(mut reg) if !reg.is_empty() => {
            match reg.initialize_all().await {
                Ok(n) => info!("MCP: initialized {} server(s)", n),
                Err(e) => warn!("MCP initialize error: {}", e),
            }
            Some(std::sync::Arc::new(reg))
        }
        Ok(_) => None,
        Err(e) => {
            warn!("MCP config error: {}", e);
            None
        }
    }
}

/// Run the gateway in the foreground (CLI `openassistant gateway`).
pub async fn start_gateway() -> Result<()> {
    info!("Starting openAssistant gateway...");
    let config = crate::config::load().await?;
    run_all(config).await
}

/// A running gateway that can be polled or stopped — used by the desktop app to
/// start/stop the server in-process.
pub struct GatewayRunHandle {
    /// The resolved bind address (host:port).
    pub addr: String,
    task: tokio::task::JoinHandle<()>,
}

impl GatewayRunHandle {
    /// Abort the gateway task (stops the WebChat server + spawned channels).
    pub fn stop(&self) {
        self.task.abort();
    }

    /// Whether the gateway task is still alive.
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    /// Await the gateway task (used by foreground callers).
    pub async fn wait(self) -> Result<()> {
        let _ = self.task.await;
        Ok(())
    }
}

/// Start the gateway on a background task and return a handle. Binds the
/// WebChat address up front so a port conflict surfaces immediately (as an
/// error) rather than disappearing into the spawned task.
pub async fn start_gateway_handle(config: Config) -> Result<GatewayRunHandle> {
    let host = crate::config::webchat_host(&config);
    let port = crate::config::webchat_port(&config);
    let addr = format!("{host}:{port}");

    // Probe the address so "port already in use" is reported to the caller.
    // The probe is dropped before the server claims it (tiny TOCTOU window,
    // acceptable for a local single-user app).
    {
        let _probe = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| anyhow::anyhow!("cannot bind {addr}: {e}"))?;
    }

    let task = tokio::spawn(async move {
        if let Err(e) = run_all(config).await {
            tracing::error!("Gateway stopped: {}", e);
        }
    });

    Ok(GatewayRunHandle { addr, task })
}

pub async fn check() -> Result<()> {
    // Check that at least one channel is configured
    let config = crate::config::load().await?;
    if config.gateway.discord_token.is_empty()
        && config.gateway.telegram_token.is_empty()
        && config.gateway.slack_token.is_empty()
    {
        return Err(anyhow::anyhow!("No messaging channels configured"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::gate;

    #[test]
    fn gate_allows_listed_users_only() {
        let allowed = vec!["U123".to_string()];
        assert!(gate("U123", &allowed, "pairing"));
        assert!(!gate("U999", &allowed, "pairing"));
    }

    #[test]
    fn gate_empty_allowlist_respects_policy() {
        // No allowlist: open policy lets anyone in; anything else ignores all.
        assert!(gate("U123", &[], "open"));
        assert!(!gate("U123", &[], "pairing"));
        assert!(!gate("U123", &[], ""));
    }

    #[test]
    fn gate_wildcard_allows_anyone() {
        let allowed = vec!["*".to_string()];
        assert!(gate("anyone", &allowed, "pairing"));
    }
}
