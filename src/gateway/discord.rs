// src/gateway/discord.rs
use anyhow::Result;
use tracing::{info, debug};

pub async fn start(_token: &str) -> Result<()> {
    info!("Discord gateway would start here");
    // Full implementation would use serenity crate
    // to connect to Discord, handle messages, reactions, etc.
    Ok(())
}

pub fn is_allowed(user_id: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return true;
    }
    allowed.iter().any(|id| id == user_id || id == "*")
}
