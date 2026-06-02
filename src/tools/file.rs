// src/tools/file.rs
use anyhow::Result;
use tracing::debug;

use crate::core::agent::ToolResult;

pub async fn execute(args: &serde_json::Value) -> Result<ToolResult> {
    let action = args["action"].as_str().unwrap_or("read");
    let path = args["path"].as_str().unwrap_or("");

    if path.is_empty() {
        return Ok(ToolResult {
            success: false,
            output: String::new(),
            error: Some("No file path provided".to_string()),
        });
    }

    debug!("File tool: {} {}", action, path);

    match action {
        "read" => {
            match tokio::fs::read_to_string(path).await {
                Ok(content) => Ok(ToolResult {
                    success: true,
                    output: content,
                    error: None,
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                }),
            }
        }
        "write" => {
            let content = args["content"].as_str().unwrap_or("");
            match tokio::fs::write(path, content).await {
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: format!("Wrote {} bytes to {}", content.len(), path),
                    error: None,
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                }),
            }
        }
        "list" => {
            match tokio::fs::read_dir(path).await {
                Ok(mut entries) => {
                    let mut files = Vec::new();
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        files.push(entry.file_name().to_string_lossy().to_string());
                    }
                    Ok(ToolResult {
                        success: true,
                        output: files.join("\n"),
                        error: None,
                    })
                }
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                }),
            }
        }
        _ => Ok(ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("Unknown file action: {}", action)),
        }),
    }
}
