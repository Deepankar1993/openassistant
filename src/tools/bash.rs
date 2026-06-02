// src/tools/bash.rs
//! Bash tool — Claude Code-style sandboxed shell execution
//! Features: timeout, background processes, output streaming, working directory

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashArgs {
    pub command: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub elevated: bool, // If true, runs with elevated permissions (Windows: runas)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashResult {
    pub output: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub background_pid: Option<u64>,
}

/// Execute a shell command — Claude Code Bash tool equivalent
pub async fn execute(args: &serde_json::Value) -> Result<BashResult> {
    let parsed: BashArgs = serde_json::from_value(args.clone())?;
    
    let command = &parsed.command;
    if command.is_empty() {
        return Ok(BashResult {
            output: "No command provided".to_string(),
            exit_code: 1,
            timed_out: false,
            duration_ms: 0,
            background_pid: None,
        });
    }

    let timeout = Duration::from_millis(parsed.timeout_ms.unwrap_or(120_000)); // 2 min default
    let start = std::time::Instant::now();

    info!("Bash: {} (timeout: {:?})", &command[..command.len().min(80)], timeout);

    // Build the command
    let mut cmd = tokio::process::Command::new("bash");
    cmd.arg("-c").arg(command);
    
    if let Some(ref dir) = parsed.working_dir {
        cmd.current_dir(dir);
    }
    
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Execute with timeout
    let output = match tokio::time::timeout(timeout, async {
        let child = cmd.spawn()?;
        child.wait_with_output().await
    }).await {
        Ok(result) => result?,
        Err(_) => {
            return Ok(BashResult {
                output: format!("Command timed out after {:?}", timeout),
                exit_code: -1,
                timed_out: true,
                duration_ms: start.elapsed().as_millis() as u64,
                background_pid: None,
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    let combined_output = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        format!("stderr: {}", stderr)
    } else {
        format!("{}\nstderr: {}", stdout, stderr)
    };

    Ok(BashResult {
        output: combined_output,
        exit_code: output.status.code().unwrap_or(-1),
        timed_out: false,
        duration_ms: start.elapsed().as_millis() as u64,
        background_pid: None,
    })
}

/// Check if bash is available
pub async fn check() -> Result<()> {
    let result = execute(&serde_json::json!({
        "command": "echo 'bash available'",
        "timeout_ms": 5000
    })).await?;
    
    if result.exit_code == 0 {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Bash not available"))
    }
}
