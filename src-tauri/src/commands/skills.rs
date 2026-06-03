//! Skills-manager commands. Lists/reads/creates skills via the real
//! `SkillEngine`. Deliberately does NOT expose `activate_skill()` — it is not
//! wired into `Agent::process`, so a UI toggle would have no effect.

use open_assistant::config;
use open_assistant::skills::engine::SkillEngine;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SkillDto {
    pub name: String,
    pub description: String,
    pub category: String,
    pub is_builtin: bool,
}

/// Load the built-in skills plus any from the configured skills dir.
fn load_engine(cfg: &config::Config) -> SkillEngine {
    let mut engine = SkillEngine::load_builtin().unwrap_or_else(|_| SkillEngine::new());
    if let Some(dir) = cfg.skills.dirs.first() {
        let _ = engine.load_from_dir(dir);
    }
    engine
}

/// List built-in and custom skills. Built-ins are those without a file path.
#[tauri::command]
pub async fn list_skills() -> Result<Vec<SkillDto>, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let engine = load_engine(&cfg);
    Ok(engine
        .list()
        .iter()
        .map(|s| SkillDto {
            name: s.name.clone(),
            description: s.description.clone(),
            category: s.category.clone(),
            is_builtin: s.file_path.is_none(),
        })
        .collect())
}

/// Return the markdown content of a named skill.
#[tauri::command]
pub async fn read_skill(name: String) -> Result<String, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let engine = load_engine(&cfg);
    engine
        .get(&name)
        .map(|s| s.content.clone())
        .ok_or_else(|| format!("Skill `{name}` not found."))
}

/// Create a new custom skill markdown file in the first configured skills dir.
#[tauri::command]
pub async fn create_skill(name: String, content: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Skill name cannot be empty.".into());
    }
    if name.contains(['/', '\\', '.']) || name.contains('\0') {
        return Err("Skill name must not contain path separators, dots, or NUL.".into());
    }
    // Windows treats these as device files regardless of extension.
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| name.eq_ignore_ascii_case(r)) {
        return Err("Skill name conflicts with a reserved device name.".into());
    }
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let dir = cfg
        .skills
        .dirs
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{}/skills", cfg.general.data_dir));
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let path = format!("{dir}/{name}.md");
    tokio::fs::write(&path, content).await.map_err(|e| e.to_string())
}
