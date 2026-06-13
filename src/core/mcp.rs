// src/core/mcp.rs
//! MCP (Model Context Protocol) client — connects to external tool servers
//! Supports stdio and HTTP transports via JSON-RPC 2.0
//! Compatible with Claude Code's MCP integration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tracing::{info, debug, warn};

/// Per-request timeout for an MCP JSON-RPC round-trip.
const MCP_TIMEOUT_SECS: u64 = 30;

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

/// A live stdio connection to an MCP server subprocess. Kept alive for the
/// client's lifetime; the child is killed on drop so we don't leak processes.
#[derive(Debug)]
struct StdioConn {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: i64,
}

impl Drop for StdioConn {
    fn drop(&mut self) {
        // Kill the server and best-effort reap so a long-running gateway that
        // replaces servers doesn't accumulate zombies (try_wait is non-blocking;
        // catches the common already-exited case).
        let _ = self.child.start_kill();
        let _ = self.child.try_wait();
    }
}

#[derive(Debug)]
pub struct McpClient {
    pub server_name: String,
    pub config: McpServerConfig,
    pub tools: Vec<McpTool>,
    initialized: bool,
    /// Live stdio process (None for HTTP servers / before init).
    conn: Option<Arc<Mutex<StdioConn>>>,
    /// Reused for HTTP transport (connection pooling).
    http: reqwest::Client,
}

impl McpClient {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            server_name: config.name.clone(),
            config,
            tools: Vec::new(),
            initialized: false,
            conn: None,
            http: reqwest::Client::new(),
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

    fn init_params() -> serde_json::Value {
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "openassistant", "version": env!("CARGO_PKG_VERSION") }
        })
    }

    async fn init_stdio(&mut self) -> Result<()> {
        let command = self.config.command.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Stdio transport requires a command"))?;
        debug!("Starting MCP stdio server: {}", command);

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
        let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?);
        self.conn = Some(Arc::new(Mutex::new(StdioConn { child, stdin, stdout, next_id: 0 })));

        // initialize → initialized notification → tools/list
        self.rpc_stdio("initialize", Self::init_params()).await?;
        if let Some(conn) = &self.conn {
            let mut g = conn.lock().await;
            let note = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
            let line = format!("{}\n", serde_json::to_string(&note)?);
            g.stdin.write_all(line.as_bytes()).await?;
            g.stdin.flush().await?;
        }
        self.tools = self.list_tools_internal().await?;
        self.initialized = true;
        info!("MCP server '{}' initialized with {} tools", self.server_name, self.tools.len());
        Ok(())
    }

    async fn init_http(&mut self) -> Result<()> {
        let url = self.config.url.as_ref()
            .ok_or_else(|| anyhow::anyhow!("HTTP transport requires a URL"))?;
        debug!("Connecting to MCP HTTP server: {}", url);
        self.rpc_http("initialize", Self::init_params()).await?;
        self.tools = self.list_tools_internal().await?;
        self.initialized = true;
        info!("MCP HTTP server '{}' initialized with {} tools", self.server_name, self.tools.len());
        Ok(())
    }

    /// One JSON-RPC round-trip over the persistent stdio process: write a line,
    /// read stdout lines until the matching id (skipping notifications / log
    /// noise), bounded by a timeout.
    async fn rpc_stdio(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let conn = self.conn.as_ref()
            .ok_or_else(|| anyhow::anyhow!("MCP server '{}' has no stdio connection", self.server_name))?;
        let mut g = conn.lock().await;
        let id = { g.next_id += 1; g.next_id };
        let req = serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let line = format!("{}\n", serde_json::to_string(&req)?);
        g.stdin.write_all(line.as_bytes()).await?;
        g.stdin.flush().await?;
        let fut = read_rpc_response(&mut g.stdout, id, &self.server_name);
        match tokio::time::timeout(std::time::Duration::from_secs(MCP_TIMEOUT_SECS), fut).await {
            Ok(r) => r,
            Err(_) => anyhow::bail!("MCP request '{}' to '{}' timed out", method, self.server_name),
        }
    }

    async fn rpc_http(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let url = self.config.url.as_ref()
            .ok_or_else(|| anyhow::anyhow!("HTTP transport requires a URL"))?;
        let req = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let resp = self.http
            .post(url)
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await?;
        let v: serde_json::Value = resp.json().await?;
        if let Some(e) = v.get("error") {
            anyhow::bail!("MCP error from '{}': {}", self.server_name, e);
        }
        Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    async fn rpc(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        match &self.config.transport {
            McpTransport::Http => self.rpc_http(method, params).await,
            McpTransport::Stdio => self.rpc_stdio(method, params).await,
            McpTransport::WebSocket => anyhow::bail!("WebSocket transport not implemented"),
        }
    }

    /// List available tools from the MCP server (cached after init).
    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        if !self.initialized {
            return Err(anyhow::anyhow!("MCP server not initialized"));
        }
        Ok(self.tools.clone())
    }

    async fn list_tools_internal(&self) -> Result<Vec<McpTool>> {
        let result = self.rpc("tools/list", serde_json::json!({})).await?;
        Ok(parse_tools_list(&result, &self.server_name))
    }

    /// Call a tool on the MCP server and return its text result.
    pub async fn call_tool(&self, tool_name: &str, arguments: serde_json::Value) -> Result<String> {
        if !self.initialized {
            return Err(anyhow::anyhow!("MCP server '{}' not initialized", self.server_name));
        }
        if !self.tools.iter().any(|t| t.name == tool_name) {
            anyhow::bail!("Tool '{}' not found on server '{}'", tool_name, self.server_name);
        }
        debug!("Calling MCP tool: {}::{}", self.server_name, tool_name);
        let result = self
            .rpc("tools/call", serde_json::json!({"name": tool_name, "arguments": arguments}))
            .await?;
        Ok(extract_call_result(&result))
    }
}

/// Read stdout lines until one parses as a JSON-RPC response with `id`,
/// skipping notifications / non-JSON log noise. Errors on EOF or JSON-RPC error.
async fn read_rpc_response(
    stdout: &mut BufReader<tokio::process::ChildStdout>,
    id: i64,
    server: &str,
) -> Result<serde_json::Value> {
    loop {
        let mut buf = String::new();
        let n = stdout.read_line(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("MCP server '{}' closed the connection", server);
        }
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue; // log line / non-JSON noise on stdout
        };
        if v.get("id").and_then(|i| i.as_i64()) == Some(id) {
            if let Some(e) = v.get("error") {
                anyhow::bail!("MCP error from '{}': {}", server, e);
            }
            return Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
        }
        // Different id or a notification — keep reading.
    }
}

/// Parse a `tools/list` result into `McpTool`s (pure; unit-tested).
pub fn parse_tools_list(result: &serde_json::Value, server: &str) -> Vec<McpTool> {
    result
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?.to_string();
                    Some(McpTool {
                        name,
                        description: t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                        input_schema: t.get("inputSchema").cloned().unwrap_or(serde_json::Value::Null),
                        server: server.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Reduce a `tools/call` result to text (pure; unit-tested): join the
/// `content[].text` items, marking errors via `isError`.
pub fn extract_call_result(result: &serde_json::Value) -> String {
    let is_error = result.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);
    let text = match result.get("content").and_then(|c| c.as_array()) {
        Some(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
                .collect();
            if parts.is_empty() {
                result.to_string()
            } else {
                parts.join("\n")
            }
        }
        None => result.to_string(),
    };
    if is_error {
        format!("⚠️ MCP tool error: {}", text)
    } else {
        text
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

    /// Load MCP servers from `<data_dir>/.mcp.json` (empty registry if absent).
    pub fn open_default(data_dir: &str) -> Result<Self> {
        Self::load_from_config(&format!("{}/.mcp.json", data_dir))
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

    /// Route a prefixed `mcp__<server>__<tool>` call to the right server.
    pub async fn call_prefixed(&self, prefixed: &str, args: serde_json::Value) -> Result<String> {
        let (server, tool) = split_prefixed(prefixed)
            .ok_or_else(|| anyhow::anyhow!("'{}' is not an mcp__server__tool name", prefixed))?;
        let client = self.servers.get(server)
            .ok_or_else(|| anyhow::anyhow!("no MCP server '{}' (is it in .mcp.json and initialized?)", server))?;
        client.call_tool(tool, args).await
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }
}

/// Split `mcp__<server>__<tool>` into (server, tool). The server is the segment
/// between `mcp__` and the next `__`; the tool is the remainder (which may
/// itself contain `__`). Returns None if not a well-formed prefixed name.
pub fn split_prefixed(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("mcp__")?;
    let idx = rest.find("__")?;
    let server = &rest[..idx];
    let tool = &rest[idx + 2..];
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tools_list_extracts_fields() {
        let v = serde_json::json!({
            "tools": [
                {"name": "search", "description": "Search the web", "inputSchema": {"type": "object"}},
                {"name": "fetch"},
                {"description": "no name — skipped"}
            ]
        });
        let tools = parse_tools_list(&v, "web");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[0].description, "Search the web");
        assert_eq!(tools[0].server, "web");
        assert_eq!(tools[1].name, "fetch");
        assert_eq!(tools[1].description, "");
        // Missing/empty tools array → empty.
        assert!(parse_tools_list(&serde_json::json!({}), "web").is_empty());
    }

    #[test]
    fn extract_call_result_joins_text_and_marks_errors() {
        let ok = serde_json::json!({"content": [{"type": "text", "text": "line1"}, {"type": "text", "text": "line2"}]});
        assert_eq!(extract_call_result(&ok), "line1\nline2");

        let err = serde_json::json!({"isError": true, "content": [{"type": "text", "text": "boom"}]});
        assert!(extract_call_result(&err).starts_with("⚠️ MCP tool error: boom"));

        // No text content → stringified fallback.
        let weird = serde_json::json!({"content": [{"type": "image"}]});
        assert!(extract_call_result(&weird).contains("image"));
    }

    #[test]
    fn split_prefixed_parses_server_and_tool() {
        assert_eq!(split_prefixed("mcp__github__search"), Some(("github", "search")));
        // Tool may itself contain `__`.
        assert_eq!(split_prefixed("mcp__fs__read__file"), Some(("fs", "read__file")));
        assert_eq!(split_prefixed("bash"), None);
        assert_eq!(split_prefixed("mcp__github"), None);
        assert_eq!(split_prefixed("mcp____search"), None); // empty server
    }
}
