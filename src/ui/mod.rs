// src/ui/mod.rs
pub mod chat;

pub async fn run_default() -> anyhow::Result<()> {
    chat::run_chat().await
}
