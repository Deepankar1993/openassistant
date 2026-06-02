// src/onboarding/mod.rs
pub mod wizard;

pub async fn run_default() -> anyhow::Result<()> {
    wizard::run_wizard().await
}
