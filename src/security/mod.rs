// src/security/mod.rs
pub mod pairing;
pub mod allowlist;

/// Security model: DM pairing and access control (OpenClaw-style)
use anyhow::Result;

pub async fn check() -> Result<()> {
    // Verify security configuration is valid
    let config = crate::config::load().await?;
    if config.security.dm_pairing && config.security.allow_from.is_empty() {
        tracing::warn!("DM pairing enabled but no users in allowlist — only paired users can interact");
    }
    Ok(())
}
