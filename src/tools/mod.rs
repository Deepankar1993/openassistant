// src/tools/mod.rs
//! Tool registry and execution

pub mod bash;
pub mod file_search;
// Re-export existing tools
pub mod shell;
pub mod file;
pub mod browser;
pub mod vision;

use anyhow::Result;
use serde_json::Value;

/// Tool registry — all available tools
pub struct ToolRegistry;

impl ToolRegistry {
    /// Execute a tool by name with the Claude Code-style tool interface
    pub async fn execute(tool_name: &str, args: &Value) -> Result<String> {
        match tool_name {
            // Claude Code tools
            "bash" => {
                let result = bash::execute(args).await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "glob" => {
                let result = file_search::glob(args).await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "grep" => {
                let result = file_search::grep(args).await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "read" | "file" => {
                let result = file::execute(args).await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "shell" => {
                let result = shell::execute(args).await?;
                Ok(result.output)
            }
            "browser" => {
                let result = browser::execute(args).await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "vision" => {
                let result = vision::execute(args).await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }
}
