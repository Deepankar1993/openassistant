// src/tools/browser.rs
use anyhow::Result;
use tracing::debug;

use crate::core::agent::ToolResult;

pub async fn execute(args: &serde_json::Value) -> Result<ToolResult> {
    let action = args["action"].as_str().unwrap_or("search");
    let query = args["query"].as_str().unwrap_or("");

    debug!("Browser tool: {} {}", action, query);

    match action {
        "search" => {
            let url = format!("https://www.google.com/search?q={}", urlencoding::encode(query));
            Ok(ToolResult {
                success: true,
                output: format!("Search results for '{}': {}", query, url),
                error: None,
            })
        }
        "browse" => {
            let client = reqwest::Client::new();
            match client.get(query).send().await {
                Ok(resp) => {
                    let text = resp.text().await.unwrap_or_default();
                    let preview = &text[..text.len().min(2000)];
                    Ok(ToolResult {
                        success: true,
                        output: format!("Content from {}:\n{}", query, preview),
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
            error: Some(format!("Unknown browser action: {}", action)),
        }),
    }
}
