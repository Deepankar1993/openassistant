// src/tools/shell.rs
use anyhow::Result;
use std::process::Command;
use tracing::debug;

use crate::core::agent::ToolResult;

pub async fn execute(args: &serde_json::Value) -> Result<ToolResult> {
    let command = args["command"].as_str().unwrap_or("").to_string();

    if command.is_empty() {
        return Ok(ToolResult {
            success: false,
            output: String::new(),
            error: Some("No command provided".to_string()),
        });
    }

    debug!("Executing shell command: {}", command);

    let result = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(&command);
        crate::core::proc::no_window_std(&mut cmd); // no console window flash on Windows
        cmd.output()
    }).await;

    let output = match result {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute command: {}", e)),
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

    Ok(ToolResult {
        success: output.status.success(),
        output: String::from_utf8_lossy(&output.stdout).to_string(),
        error: if output.status.success() {
            None
        } else {
            Some(String::from_utf8_lossy(&output.stderr).to_string())
        },
    })
}
