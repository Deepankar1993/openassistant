//! System commands: diagnostics, external links, read-only agent definitions.

use open_assistant::config;
use open_assistant::core::memory::MemoryWorkspace;
use open_assistant::core::subagent::SubAgentOrchestrator;
use open_assistant::memory::store::MemoryStore;
use open_assistant::skills::engine::SkillEngine;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Serialize)]
pub struct DiagnosticResultDto {
    pub name: String,
    pub ok: bool,
    pub message: String,
    /// Optional checks render amber (not red) when failing.
    pub is_optional: bool,
}

#[derive(Debug, Serialize)]
pub struct AgentDto {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub model: Option<String>,
}

fn result(name: &str, ok: bool, message: impl Into<String>, is_optional: bool) -> DiagnosticResultDto {
    DiagnosticResultDto { name: name.into(), ok, message: message.into(), is_optional }
}

/// Run the same six diagnostics as `cargo run -- doctor`, collected as structured
/// rows. The skills check uses `load_builtin()` (returns 3) rather than the CLI's
/// `default()` (returns 0). Vision/Gateway are optional (amber on failure).
#[tauri::command(rename_all = "snake_case")]
pub async fn run_doctor() -> Result<Vec<DiagnosticResultDto>, String> {
    let mut out = Vec::new();

    match config::check().await {
        Ok(_) => out.push(result("Config", true, "Loaded successfully", false)),
        Err(e) => out.push(result("Config", false, e.to_string(), false)),
    }

    match MemoryStore::open_default().await {
        Ok(_) => out.push(result("Memory database", true, "SQLite + FTS5 OK", false)),
        Err(e) => out.push(result("Memory database", false, e.to_string(), false)),
    }

    let cfg = config::load().await.map_err(|e| e.to_string())?;
    match MemoryWorkspace::from_data_dir(&cfg.general.data_dir).init() {
        Ok(_) => out.push(result("Memory workspace", true, "Files initialized", false)),
        Err(e) => out.push(result("Memory workspace", false, e.to_string(), false)),
    }

    match SkillEngine::load_builtin() {
        Ok(engine) => {
            let n = engine.count();
            out.push(result("Skills", true, format!("{n} built-in skills loaded"), false));
        }
        Err(e) => out.push(result("Skills", false, e.to_string(), false)),
    }

    match open_assistant::gateway::check().await {
        Ok(_) => out.push(result("Gateway", true, "Configured", true)),
        Err(e) => out.push(result("Gateway", false, e.to_string(), true)),
    }

    // Vision is an external process probe; cap it so a missing binary on Windows
    // doesn't hang the panel.
    let vision = tokio::time::timeout(Duration::from_millis(800), open_assistant::tools::vision::check()).await;
    match vision {
        Ok(Ok(_)) => out.push(result("Vision (Gemini CLI)", true, "Detected", true)),
        Ok(Err(_)) => out.push(result(
            "Vision (Gemini CLI)",
            false,
            "Not found — image analysis unavailable",
            true,
        )),
        Err(_) => out.push(result("Vision (Gemini CLI)", false, "Check timed out", true)),
    }

    Ok(out)
}

#[derive(Debug, Serialize)]
pub struct GatewayRequirementDto {
    pub name: String,
    pub ok: bool,
    pub required: bool,
    pub detail: String,
}

/// Gateway setup readiness, surfaced in the Settings → Channels panel. Mirrors
/// the CLI's `openassistant gateway --check` output (shared core function).
#[tauri::command(rename_all = "snake_case")]
pub async fn gateway_readiness() -> Result<Vec<GatewayRequirementDto>, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    Ok(open_assistant::gateway::readiness(&cfg)
        .into_iter()
        .map(|r| GatewayRequirementDto { name: r.name, ok: r.ok, required: r.required, detail: r.detail })
        .collect())
}

/// Open a provider-documentation URL in the OS browser (not the webview).
/// The frontend only ever passes URLs matching the opener capability allowlist.
#[tauri::command(rename_all = "snake_case")]
pub async fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    // Defense in depth: the opener capability scope only constrains the plugin's
    // own JS API, not this custom command. A compromised webview could invoke us
    // with file:// or javascript: — so validate the scheme in Rust too.
    if !url.starts_with("https://") {
        return Err("Only https:// URLs may be opened.".into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// List sub-agent definitions (read-only). Execution is intentionally NOT exposed
/// — `execute_subagent()` returns a hardcoded placeholder regardless of input.
#[tauri::command(rename_all = "snake_case")]
pub async fn list_agents() -> Result<Vec<AgentDto>, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let mut orch = SubAgentOrchestrator::new();
    let dir = format!("{}/.claude/agents", cfg.general.data_dir);
    let _ = orch.load_definitions(&dir); // missing dir is fine -> empty list
    Ok(orch
        .list_definitions()
        .into_iter()
        .map(|d| AgentDto {
            name: d.name.clone(),
            description: d.description.clone(),
            tools: d.tools.clone().unwrap_or_default(),
            model: d.model.clone(),
        })
        .collect())
}
