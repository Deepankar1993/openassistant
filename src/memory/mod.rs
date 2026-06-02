// src/memory/mod.rs
pub mod store;
pub mod search;

use anyhow::Result;

pub async fn check() -> Result<()> {
    let _ = store::MemoryStore::open_default().await?;
    Ok(())
}
