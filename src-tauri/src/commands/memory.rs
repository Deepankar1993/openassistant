//! Memory-browser commands. The markdown notes (`MEMORY.md`, daily files) are
//! backed by `MemoryWorkspace` file I/O; the discrete "what I know about you"
//! facts are backed by the `MemoryStore` SQLite db (`<data_dir>/memory.db`).

use open_assistant::config;
use open_assistant::core::memory::MemoryWorkspace;
use open_assistant::memory::store::{MemoryEntry, MemoryStore};

async fn workspace() -> Result<MemoryWorkspace, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    Ok(MemoryWorkspace::from_data_dir(&cfg.general.data_dir))
}

async fn facts_store() -> Result<MemoryStore, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    MemoryStore::open_in(&cfg.general.data_dir).map_err(|e| e.to_string())
}

fn slug_key(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() { "fact".to_string() } else { slug.chars().take(48).collect() }
}

/// Long-term curated memory (`MEMORY.md`).
#[tauri::command(rename_all = "snake_case")]
pub async fn get_memory_md() -> Result<String, String> {
    Ok(workspace().await?.read_long_term())
}

/// Overwrite `MEMORY.md`.
#[tauri::command(rename_all = "snake_case")]
pub async fn write_memory_md(content: String) -> Result<(), String> {
    workspace().await?.write_long_term(&content).map_err(|e| e.to_string())
}

/// Today's daily note (read-only in the UI).
#[tauri::command(rename_all = "snake_case")]
pub async fn get_today_note() -> Result<String, String> {
    Ok(workspace().await?.read_today())
}

/// Search memory markdown files; returns `[filename, excerpt]` pairs.
#[tauri::command(rename_all = "snake_case")]
pub async fn search_memory_files(query: String) -> Result<Vec<[String; 2]>, String> {
    let ws = workspace().await?;
    Ok(ws
        .search_files(&query)
        .into_iter()
        .map(|(name, excerpt)| [name, excerpt])
        .collect())
}

// ── "What I know about you" — durable per-item facts (MemoryStore) ──

/// All remembered facts, most important / most recent first.
#[tauri::command(rename_all = "snake_case")]
pub async fn list_user_facts() -> Result<Vec<MemoryEntry>, String> {
    facts_store().await?.list_all(200).map_err(|e| e.to_string())
}

/// Add a fact the user typed themselves (source = "manual").
#[tauri::command(rename_all = "snake_case")]
pub async fn add_user_fact(
    value: String,
    category: Option<String>,
    importance: Option<f64>,
) -> Result<(), String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err("A fact can't be empty.".into());
    }
    let entry = MemoryEntry::new(
        slug_key(&value),
        value,
        category.unwrap_or_else(|| "fact".to_string()),
        "manual",
        importance.unwrap_or(0.6),
    );
    facts_store().await?.store(&entry).map(|_| ()).map_err(|e| e.to_string())
}

/// Edit a fact's text and importance.
#[tauri::command(rename_all = "snake_case")]
pub async fn update_user_fact(id: i64, value: String, importance: f64) -> Result<(), String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err("A fact can't be empty.".into());
    }
    facts_store().await?.update(id, &value, importance).map(|_| ()).map_err(|e| e.to_string())
}

/// One-click forget.
#[tauri::command(rename_all = "snake_case")]
pub async fn delete_user_fact(id: i64) -> Result<(), String> {
    facts_store().await?.delete_by_id(id).map(|_| ()).map_err(|e| e.to_string())
}
