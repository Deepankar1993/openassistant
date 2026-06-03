//! Persona ("SOUL") commands — view and edit the agent's identity, which is
//! rendered into every system prompt via `FullContext::build_system_prompt`.
//! Persisted to `<data_dir>/persona.json` and applied live to the running agent.

use crate::state::AppCore;
use open_assistant::config;
use open_assistant::core::persona::Persona;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Editable subset of the persona. Non-editable fields (version, preferences)
/// are preserved from the persisted persona on save.
#[derive(Debug, Serialize, Deserialize)]
pub struct PersonaDto {
    pub name: String,
    pub emoji: String,
    pub tone: String,
    pub language: String,
    pub personality: String,
    pub principles: Vec<String>,
    pub boundaries: Vec<String>,
    pub capabilities: Vec<String>,
}

impl PersonaDto {
    fn from_persona(p: &Persona) -> Self {
        Self {
            name: p.name.clone(),
            emoji: p.emoji.clone(),
            tone: p.tone.clone(),
            language: p.language.clone(),
            personality: p.personality.clone(),
            principles: p.principles.clone(),
            boundaries: p.boundaries.clone(),
            capabilities: p.capabilities.clone(),
        }
    }
}

fn clean_list(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Read the current persona (persisted or default) for the Persona settings.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_persona() -> Result<PersonaDto, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let persona = Persona::load_or_default(&cfg.general.data_dir);
    Ok(PersonaDto::from_persona(&persona))
}

/// Persist edits to the persona and apply them to the live agent immediately
/// (the next message uses the updated identity).
#[tauri::command(rename_all = "snake_case")]
pub async fn save_persona(state: State<'_, AppCore>, dto: PersonaDto) -> Result<(), String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let mut turn = state.turn.lock().await;

    // Start from the persisted persona to keep non-editable fields intact.
    let mut persona = Persona::load_or_default(&cfg.general.data_dir);
    persona.name = dto.name.trim().to_string();
    persona.emoji = dto.emoji.trim().to_string();
    persona.tone = dto.tone.trim().to_string();
    persona.language = dto.language.trim().to_string();
    persona.personality = dto.personality.trim().to_string();
    persona.principles = clean_list(dto.principles);
    persona.boundaries = clean_list(dto.boundaries);
    persona.capabilities = clean_list(dto.capabilities);

    persona.save(&cfg.general.data_dir).map_err(|e| e.to_string())?;
    turn.ctx.persona = persona;
    Ok(())
}
