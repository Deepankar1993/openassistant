// src/core/mcp.rs
//! MCP (Model Context Protocol) client — connects to external tool servers
//! Supports stdio and HTTP transports via JSON-RPC 2.0
//! Compatible with Claude Code's MCP integration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, debug, warn};

// ─── MCP Server Definition (from .mcp.json) ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransport,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpTransport {
    Stdio,
    Http,
    WebSocket,
}

// ─── MCP Tool Definition ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server: String,
}

// ─── MCP Client ───────────────────────────────────────────────────────

#[derive(Debug)]
pub struct McpClient {
    pub server_name: String,
    pub config: McpServerConfig,
    pub tools: Vec<McpTool>,
    initialized: bool,
}

impl McpClient {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            server_name: config.name.clone(),
            config,
            tools: Vec::new(),
            initialized: false,
        }
    }

    /// Initialize the MCP server connection
    pub async fn initialize(&mut self) -> Result<()> {
        if !self.config.enabled {
            warn!("MCP server '{}' is disabled", self.server_name);
            return Ok(());
        }

        info!("Initializing MCP server: {}", self.server_name);

        match &self.config.transport {
            McpTransport::Stdio => self.init_stdio().await,
            McpTransport::Http => self.init_http().await,
            McpTransport::WebSocket => {
                warn!("WebSocket transport not yet implemented for '{}'", self.server_name);
                Ok(())
            }
        }
    }

    async fn init_stdio(&mut self) -> Result<()> {
        let command = self.config.command.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Stdio transport requires a command"))?;

        debug!("Starting MCP stdio server: {}", command);

        // Build the JSON-RPC initialize request
        let init_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "openassistant",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        // Spawn the server process
        let mut cmd = tokio::process::Command::new(command);
        if let Some(ref args) = self.config.args {
            cmd.args(args);
        }
        if let Some(ref env) = self.config.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn()?;

        // Send initialize request
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let init_json = serde_json::to_string(&init_request)?;
            stdin.write_all(init_json.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
        }

        // Read response
        if let Some(stdout) = child.stdout.take() {
            use tokio::io::AsyncBufReadExt;
            let reader = tokio::io::BufReader::new(stdout);
            if let Some(Ok(line)) = reader.lines().next_line().await.transpose() {
                debug!("MCP init response: {}", line);
            }
        }

        // Send initialized notification
        let _initialized = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        // List available tools
        self.tools = self.list_tools_internal().await?;
        self.initialized = true;

        info!("MCP server '{}' initialized with {} tools", self.server_name, self.tools.len());
        Ok(())
    }

    async fn init_http(&mut self) -> Result<()> {
        let url = self.config.url.as_ref()
            .ok_or_else(|| anyhow::anyhow!("HTTP transport requires a URL"))?;

        debug!("Connecting to MCP HTTP server: {}", url);

        let client = reqwest::Client::new();
        let init_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "openassistant",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        let resp = client
            .post(url)
            .header("Content-Type", "application/json")
            .json(&init_request)
            .send()
            .await?;

        let _result: serde_json::Value = resp.json().await?;
        self.tools = self.list_tools_internal().await?;
        self.initialized = true;

        info!("MCP HTTP server '{}' initialized with {} tools", self.server_name, self.tools.len());
        Ok(())
    }

    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        if !self.initialized {
            return Err(anyhow::anyhow!("MCP server not initialized"));
        }
        Ok(self.tools.clone())
    }

    async fn list_tools_internal(&self) -> Result<Vec<McpTool>> {
        // In a full implementation, this would send a tools/list JSON-RPC request
        // For now, return empty (real implementation would parse the response)
        Ok(Vec::new())
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(&self, tool_name: &str, arguments: serde_json::Value) -> Result<String> {
        if !self.initialized {
            return Err(anyhow::anyhow!("MCP server '{}' not initialized", self.server_name));
        }

        let tool = self.tools.iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found on server '{}'", tool_name, self.server_name))?;

        debug!("Calling MCP tool: {}::{}", self.server_name, tool_name);

        let call_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        match &self.config.transport {
            McpTransport::Http => {
                let url = self.config.url.as_ref().unwrap();
                let client = reqwest::Client::new();
                let resp = client
                    .post(url)
                    .header("Content-Type", "application/json")
                    .json(&call_request)
                    .send()
                    .await?;

                let result: serde_json::Value = resp.json().await?;
                Ok(result.to_string())
            }
            _ => {
                // Stdio would write to stdin and read from stdout
                Ok(format!("MCP tool '{}' called (stdio not fully implemented)", tool_name))
            }
        }
    }
}

// ─── MCP Registry ─────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct McpRegistry {
    servers: HashMap<String, McpClient>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load MCP servers from .mcp.json config file
    pub fn load_from_config(path: &str) -> Result<Self> {
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            debug!("MCP config file not found: {}", path);
            return Ok(Self::new());
        }

        let content = std::fs::read_to_string(&path_buf)?;
        let config: serde_json::Value = serde_json::from_str(&content)?;

        let mut registry = Self::new();

        if let Some(servers) = config.get("mcpServers").and_then(|s| s.as_object()) {
            for (name, server_config) in servers {
                if let Ok(transport_str) = serde_json::from_value::<String>(
                    server_config.get("transport").unwrap_or(&serde_json::json!("stdio")).clone()
                ) {
                    let transport = match transport_str.as_str() {
                        "http" => McpTransport::Http,
                        "websocket" => McpTransport::WebSocket,
                        _ => McpTransport::Stdio,
                    };

                    let config = McpServerConfig {
                        name: name.clone(),
                        transport,
                        command: server_config.get("command").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        args: server_config.get("args").and_then(|v| v.as_array()).map(|arr| {
                            arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect()
                        }),
                        env: server_config.get("env").and_then(|v| v.as_object()).map(|obj| {
                            obj.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect()
                        }),
                        url: server_config.get("url").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        enabled: server_config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                    };

                    registry.servers.insert(name.clone(), McpClient::new(config));
                }
            }
        }

        info!("Loaded {} MCP servers from {}", registry.servers.len(), path);
        Ok(registry)
    }

    pub async fn initialize_all(&mut self) -> Result<usize> {
        let mut count = 0;
        for (_, client) in self.servers.iter_mut() {
            if let Err(e) = client.initialize().await {
                warn!("Failed to initialize MCP server '{}': {}", client.server_name, e);
            } else {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn get_server(&self, name: &str) -> Option<&McpClient> {
        self.servers.get(name)
    }

    pub fn list_servers(&self) -> Vec<&McpClient> {
        self.servers.values().collect()
    }

    /// Get all tools from all servers (prefixed with mcp__<server>__<tool>)
    pub fn list_all_tools(&self) -> Vec<McpTool> {
        let mut all_tools = Vec::new();
        for (_, server) in &self.servers {
            for tool in &server.tools {
                let mut prefixed = tool.clone();
                prefixed.name = format!("mcp__{}__{}", server.server_name, tool.name);
                all_tools.push(prefixed);
            }
        }
        all_tools
    }
}
