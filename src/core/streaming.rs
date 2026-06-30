// src/core/streaming.rs
//! Streaming output — NDJSON (newline-delimited JSON) format
//! Compatible with Claude Code's --output-format stream-json

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncRead;

// ─── Streaming Output Events ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// System initialization
    System {
        subtype: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// API request retry
    ApiRetry {
        attempt: u32,
        max_retries: u32,
        error: String,
    },
    /// Stream event with delta content
    StreamEvent {
        #[serde(skip_serializing_if = "Option::is_none")]
        event: Option<StreamDelta>,
    },
    /// Tool call in progress
    ToolCall {
        tool_name: String,
        tool_input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_output: Option<String>,
    },
    /// Final result
    Result {
        subtype: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        num_turns: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total_cost_usd: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
    },
    /// Error
    Error {
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDelta {
    #[serde(rename = "type")]
    pub delta_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

// ─── Streaming Session ────────────────────────────────────────────────

#[derive(Debug)]
pub struct StreamingSession {
    pub session_id: String,
    pub events: Vec<StreamEvent>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost: f64,
}

impl StreamingSession {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            events: Vec::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cost: 0.0,
        }
    }

    pub fn add_event(&mut self, event: StreamEvent) {
        self.events.push(event);
    }

    /// Extract only text deltas from the stream
    pub fn extract_text(&self) -> String {
        let mut text = String::new();
        for event in &self.events {
            if let StreamEvent::StreamEvent { event: Some(delta) } = event {
                if let Some(ref t) = delta.text {
                    text.push_str(t);
                }
            }
        }
        text
    }

    /// Format as NDJSON (newline-delimited JSON)
    pub fn to_ndjson(&self) -> String {
        self.events
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Parse NDJSON into StreamEvents
    pub fn from_ndjson(input: &str) -> Vec<StreamEvent> {
        input
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return None;
                }
                serde_json::from_str::<StreamEvent>(trimmed).ok()
            })
            .collect()
    }

    pub fn format_summary(&self) -> String {
        format!(
            "📊 Stream Summary: {} events, {} input tokens, {} output tokens, ${:.4}",
            self.events.len(),
            self.total_input_tokens,
            self.total_output_tokens,
            self.total_cost
        )
    }
}

// ─── Streaming Tool Output ────────────────────────────────────────────

/// Stream bash command output line by line
pub async fn stream_bash_output(
    command: &str,
    timeout_ms: u64,
) -> Result<Vec<String>> {
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let output = tokio::time::timeout(timeout, async {
        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c")
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true); // don't orphan the child if the timeout fires
        crate::core::proc::no_window(&mut cmd); // no console window flash on Windows
        let child = cmd.spawn()?;
        child.wait_with_output().await
    }).await??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().map(|s| s.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ndjson_roundtrip() {
        let mut session = StreamingSession::new("test-123");
        session.add_event(StreamEvent::System {
            subtype: "init".to_string(),
            session_id: Some("test-123".to_string()),
        });
        session.add_event(StreamEvent::StreamEvent {
            event: Some(StreamDelta {
                delta_type: "text_delta".to_string(),
                text: Some("Hello world".to_string()),
                partial_json: None,
            }),
        });

        let ndjson = session.to_ndjson();
        let parsed = StreamingSession::from_ndjson(&ndjson);
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn test_extract_text() {
        let mut session = StreamingSession::new("test");
        session.add_event(StreamEvent::StreamEvent {
            event: Some(StreamDelta {
                delta_type: "text_delta".to_string(),
                text: Some("Hello ".to_string()),
                partial_json: None,
            }),
        });
        session.add_event(StreamEvent::StreamEvent {
            event: Some(StreamDelta {
                delta_type: "text_delta".to_string(),
                text: Some("world".to_string()),
                partial_json: None,
            }),
        });

        assert_eq!(session.extract_text(), "Hello world");
    }
}
