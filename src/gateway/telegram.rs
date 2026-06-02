// src/gateway/telegram.rs
use anyhow::Result;
use tracing::info;

pub async fn start(_token: &str) -> Result<()> {
    info!("Telegram gateway would start here");
    Ok(())
}
