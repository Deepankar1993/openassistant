// src/gateway/mod.rs
pub mod discord;
pub mod telegram;
pub mod slack;
pub mod webchat;

use anyhow::Result;
use tracing::info;

pub async fn start_gateway() -> Result<()> {
    info!("Starting openAssistant gateway...");
    info!("Loading config...");

    let config = crate::config::load().await?;

    // Start Discord if configured. Spawned (not awaited) so it runs alongside
    // WebChat; a failure on that task is logged rather than silently lost.
    if !config.gateway.discord_token.is_empty() {
        info!("Discord token configured, starting Discord handler...");
        let cfg = config.clone();
        tokio::spawn(async move {
            if let Err(e) = discord::start(cfg).await {
                tracing::error!("Discord gateway error: {}", e);
            }
        });
    }

    // Start Telegram if configured
    if !config.gateway.telegram_token.is_empty() {
        info!("Telegram token configured, starting Telegram handler...");
    }

    // Start WebChat
    info!("Starting Web Chat on port {}", config.gateway.webhook_port);
    webchat::start(config.gateway.webhook_port).await?;

    Ok(())
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
