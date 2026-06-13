# MCP Tool Invocation — Design (cluster batch 4 of 4)

Date: 2026-06-14
Status: Approved (cluster continuation; user: "continue")

## Context

`src/core/mcp.rs` has the scaffolding (config parse from `.mcp.json`, `McpRegistry`,
prefixed `list_all_tools` → `mcp__<server>__<tool>`) but the transports are not real:
`init_stdio` spawns a child, reads one line, then **drops the process**;
`list_tools_internal` returns `Ok(vec![])`; `call_tool` over stdio is a stub; HTTP
`call_tool` returns the raw JSON. And nothing is wired into the agent. This batch makes
the client real and lets the agent actually call MCP tools.

## Architecture

### 1. Real transports (`src/core/mcp.rs`)

**Persistent stdio connection.** `McpClient` gains
`conn: Option<Arc<tokio::sync::Mutex<StdioConn>>>` where
`StdioConn { child, stdin, stdout: BufReader<ChildStdout>, next_id }`. The `Child` is
kept alive for the client's lifetime (killed on drop).

`rpc_request(&self, method, params) -> Result<Value>` (stdio): lock the conn, write one
line of JSON-RPC with an incrementing id, then read stdout lines until one parses as
JSON with the matching `id` (skipping notifications/log lines), all under a 30s
`tokio::time::timeout`. Returns the `result` or an error from `error`.

- `init_stdio`: spawn, store the `StdioConn`, `rpc_request("initialize", …)`, write the
  `notifications/initialized` line (no response), then `tools/list`.
- `init_http`: POST `initialize`, then `tools/list` (already mostly there).
- `list_tools_internal`: stdio → `rpc_request("tools/list")`; http → POST. Both feed the
  pure helper `parse_tools_list(&Value) -> Vec<McpTool>`.
- `call_tool`: stdio → `rpc_request("tools/call", {name, arguments})`; http → POST. Both
  feed the pure helper `extract_call_result(&Value) -> String` (joins
  `result.content[].text`, or stringifies, surfacing `isError`).

Pure helpers `parse_tools_list` and `extract_call_result` are unit-tested against sample
MCP JSON (the subprocess round-trip itself isn't unit-tested — no cross-platform mock
server; exercised via the CLI).

### 2. Registry (`src/core/mcp.rs`)

- `open_default(data_dir) -> Result<Self>` — loads `<data_dir>/.mcp.json` (the existing
  `load_from_config`).
- `call_prefixed(&self, prefixed, args) -> Result<String>` — split
  `mcp__<server>__<tool>` (pure helper `split_prefixed(&str) -> Option<(server, tool)>`,
  unit-tested), look up the server, `call_tool`. Errors if unknown.

### 3. Agent integration (`src/core/agent.rs`)

- `Agent.mcp: Option<Arc<McpRegistry>>` (default None) + `.with_mcp(Arc<McpRegistry>)`.
- `build_system_prompt`: if `mcp` is set, append the registry's `list_all_tools()` as
  `- **mcp__server__tool**: <description>` lines under the tool list (so the model knows
  they exist).
- `execute_tool`: a `name if name.starts_with("mcp__")` arm → `self.mcp` →
  `registry.call_prefixed(name, args)`; no registry / unknown → readable text.
- Permission gating: MCP tools flow through `check_permission` by their full
  `mcp__server__tool` name. They are NOT auto-allowed (could be a filesystem-write
  server). So locally (`BypassPermissions`) they run; on a gateway (`acceptEdits`) they
  hit `Ask` → headless refusal unless the operator adds a `permissions.allow: ["mcp__*"]`
  rule (the rule matcher already supports `*`). Documented.
- The regex `\[TOOL:(\w+):…` matches `mcp__server__tool` (all word chars). No parser change.

### 4. Wiring (build the registry once; it owns subprocesses)

The registry holds live subprocesses, so it's built **once** and shared `Arc`:
- Gateway: `run_all` builds `McpRegistry::open_default(data_dir)`, `initialize_all().await`,
  wraps in `Arc`, and passes it to `build_state` → the agent (`build_state` gains an
  `Option<Arc<McpRegistry>>` param).
- TUI/CLI chat (`ui/chat.rs`): build + init + `.with_mcp(...)` for the local agent.
- Desktop: deferred (its agent is built sync in `build_core`; adding async MCP init is a
  follow-up) — noted, not wired this batch.

### 5. CLI (`src/main.rs`)

`openassistant mcp --action list|call`:
- `list`: load + `initialize_all`, print each server and its tools.
- `call --server <s> --tool <t> --args '<json>'`: init that server, call, print result.
Lets the operator verify a server without the gateway.

## Non-goals

- WebSocket transport (still a logged warning).
- Streamable-HTTP/SSE niceties — HTTP is best-effort simple JSON-RPC POST; stdio is the
  primary, fully-real path (matches the common npx-style local MCP servers).
- Desktop MCP wiring (follow-up).

## Error handling

A server that fails to init is logged and skipped (`initialize_all` already does this); a
dead stdio process surfaces as a tool error the model sees; request timeouts return an
error string, never hang the turn.

## Testing

- `parse_tools_list`: extracts name/description/input_schema from a sample
  `{"tools":[…]}`; empty/missing → empty.
- `extract_call_result`: joins `content[].text`; surfaces `isError`; stringifies
  non-text.
- `split_prefixed`: `mcp__github__search` → `("github","search")`; non-prefixed → None;
  server/tool with embedded `__` handled (split on first/last sensibly — define: server
  is the segment between `mcp__` and the next `__`, tool is the remainder).
- `cargo test --workspace` green; clippy no new warnings; `mcp --action list` smoke.
