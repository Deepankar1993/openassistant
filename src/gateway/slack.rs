// src/gateway/slack.rs
use anyhow::Result;
use tracing::info;

pub async fn start(_token: &str) -> Result<()> {
    info!("Slack gateway would start here");
    Ok(())
}
