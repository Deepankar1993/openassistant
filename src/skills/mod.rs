// src/skills/mod.rs
pub mod engine;

use anyhow::Result;

pub async fn check() -> Result<usize> {
    let engine = engine::SkillEngine::default();
    Ok(engine.count())
}
