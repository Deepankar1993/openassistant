// src/tools/vision.rs
use anyhow::Result;
use std::process::Command;
use tracing::{info, debug};

use crate::core::agent::ToolResult;

/// Execute vision analysis using the Gemini CLI directly
pub async fn execute(args: &serde_json::Value) -> Result<ToolResult> {
    let image_path = args["image_path"].as_str().unwrap_or("");
    let question = args["question"].as_str().unwrap_or("Describe this image in detail.");

    info!("Analyzing image with Gemini CLI: {}", image_path);

    if image_path.is_empty() {
        return Ok(ToolResult {
            success: false,
            output: String::new(),
            error: Some("No image path provided".to_string()),
        });
    }

    let prompt = format!("[image: {}] {}", image_path, question);
    let result = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("gemini");
        cmd.arg("--skip-trust").arg("image").arg("analyze").arg(&prompt);
        crate::core::proc::no_window_std(&mut cmd); // no console window flash on Windows
        cmd.output()
    }).await;

    let output = match result {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute gemini: {}", e)),
            });
        }
        Err(e) => {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Task join error: {}", e)),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        let cleaned = stdout
            .lines()
            .filter(|l| !l.starts_with("Warning:") && !l.starts_with("Ripgrep"))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ToolResult {
            success: true,
            output: cleaned.trim().to_string(),
            error: None,
        })
    } else {
        Ok(ToolResult {
            success: false,
            output: stdout,
            error: Some(stderr),
        })
    }
}

/// Check if Gemini CLI is available
pub async fn check() -> Result<()> {
    debug!("Checking Gemini CLI availability");
    let result = tokio::task::spawn_blocking(|| {
        let mut cmd = Command::new("gemini");
        cmd.arg("--skip-trust").arg("whoami").env("GEMINI_CLI_TRUST_WORKSPACE", "true");
        crate::core::proc::no_window_std(&mut cmd); // no console window flash on Windows
        cmd.output()
    }).await;

    let output = match result {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to run gemini: {}", e)),
        Err(e) => return Err(anyhow::anyhow!("Task join error: {}", e)),
    };

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Gemini CLI not available: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
