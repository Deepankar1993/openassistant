//! Memory-browser commands — all backed by the real `MemoryWorkspace` file I/O.

use open_assistant::config;
use open_assistant::core::memory::MemoryWorkspace;

async fn workspace() -> Result<MemoryWorkspace, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    Ok(MemoryWorkspace::from_data_dir(&cfg.general.data_dir))
}

/// Long-term curated memory (`MEMORY.md`).
#[tauri::command]
pub async fn get_memory_md() -> Result<String, String> {
    Ok(workspace().await?.read_long_term())
}

/// Overwrite `MEMORY.md`.
#[tauri::command]
pub async fn write_memory_md(content: String) -> Result<(), String> {
    workspace().await?.write_long_term(&content).map_err(|e| e.to_string())
}

/// Today's daily note (read-only in the UI).
#[tauri::command]
pub async fn get_today_note() -> Result<String, String> {
    Ok(workspace().await?.read_today())
}

/// Search memory markdown files; returns `[filename, excerpt]` pairs.
#[tauri::command]
pub async fn search_memory_files(query: String) -> Result<Vec<[String; 2]>, String> {
    let ws = workspace().await?;
    Ok(ws
        .search_files(&query)
        .into_iter()
        .map(|(name, excerpt)| [name, excerpt])
        .collect())
}
