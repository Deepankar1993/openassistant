// src/core/agent.rs
//! Agent engine — the brain of openAssistant
//! Wires together: LLM calling, tool execution, memory, persona, daily notes

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, debug};

use super::persona::FullContext;
use super::session::Session;
use super::Message;

#[derive(Debug, Clone)]
pub struct Agent {
    pub model: String,
    pub tools: Vec<ToolDefinition>,
    pub workspace_dir: String,
    /// When false, the agent skips tool dispatch entirely and returns the raw
    /// LLM text. Defaults to `true` for the CLI; the desktop app constructs the
    /// agent with this off so a packaged binary never hands the model ungated
    /// shell/file access without explicit user consent. See openspec change
    /// `add-desktop-app`, task 2.10.
    pub tools_enabled: bool,
    /// Permission posture for tool dispatch. Local front-ends keep the
    /// pre-existing full-autonomy behavior (BypassPermissions); gateway
    /// channels cap this via `with_permission_mode` (origin-aware, like the
    /// claude bridge). Config deny/ask rules apply at EVERY mode.
    pub permission_mode: super::permissions::PermissionMode,
    /// Sub-agent nesting depth: 0 = top-level. Sub-agents get depth+1 and
    /// refuse to spawn further sub-agents.
    pub depth: u8,
    /// True only for the trusted local operator (CLI/TUI/desktop). Gates
    /// arbitrary-shell features like lifecycle hooks: a turn driven by a remote
    /// channel (Discord/Telegram/Slack) must NEVER fire the host's shell hooks.
    /// Default false (untrusted); local front-ends opt in via `.operator()`.
    pub operator: bool,
    /// Optional registry of initialized MCP servers. Shared (`Arc`) because it
    /// owns live subprocesses; built once at startup and attached via
    /// `.with_mcp()`. When set, its tools are advertised in the prompt as
    /// `mcp__<server>__<tool>` and routed in `execute_tool`.
    pub mcp: Option<std::sync::Arc<super::mcp::McpRegistry>>,
}

/// Max LLM⇄tool rounds per user turn.
const MAX_TOOL_ITERATIONS: usize = 6;
/// Max bytes of a single tool output fed back to the model.
const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;

/// Streaming events emitted during an agent turn. The JSON shape (`type` tag,
/// snake_case) is a frozen contract consumed by the WebChat SSE client and the
/// desktop `chat-event` listener — change it only with both frontends.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Token { text: String },
    ToolStart { name: String, summary: String },
    ToolEnd { name: String, ok: bool, preview: String },
    Done { text: String },
    Error { message: String },
}

/// One parsed SSE line from a chat-completions stream.
#[derive(Debug)]
pub(crate) enum SseLine {
    Done,
    Json(serde_json::Value),
}

/// Parse a single SSE line: only `data:` lines matter; `[DONE]` ends the
/// stream; comments/event lines/garbage are ignored.
pub(crate) fn parse_sse_line(line: &str) -> Option<SseLine> {
    let data = line.trim().strip_prefix("data:")?.trim();
    if data == "[DONE]" {
        return Some(SseLine::Done);
    }
    serde_json::from_str::<serde_json::Value>(data).ok().map(SseLine::Json)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl Agent {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            tools: Self::default_tools(),
            workspace_dir: crate::config::data_dir_default(),
            tools_enabled: true,
            permission_mode: super::permissions::PermissionMode::BypassPermissions,
            depth: 0,
            operator: false,
            mcp: None,
        }
    }

    /// Attach an initialized MCP registry (its tools become callable as
    /// `mcp__<server>__<tool>`).
    pub fn with_mcp(mut self, registry: std::sync::Arc<super::mcp::McpRegistry>) -> Self {
        self.mcp = Some(registry);
        self
    }

    /// Mark this agent as the trusted local operator — enables lifecycle hooks
    /// (which run arbitrary shell). Called by the CLI/TUI/desktop front-ends;
    /// never by the gateway channels.
    pub fn operator(mut self) -> Self {
        self.operator = true;
        self
    }

    pub fn with_workspace(mut self, dir: impl Into<String>) -> Self {
        self.workspace_dir = dir.into();
        self
    }

    /// Enable or disable tool dispatch. Front-ends (e.g. the desktop app) use
    /// this to keep tool execution off by default until the user opts in.
    pub fn with_tools_enabled(mut self, enabled: bool) -> Self {
        self.tools_enabled = enabled;
        self
    }

    /// Cap the permission mode for this agent's tool dispatch (origin-aware:
    /// remote gateway channels pass a stricter mode than local front-ends).
    pub fn with_permission_mode(mut self, mode: super::permissions::PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    /// Key used for rule matching: bash/shell check as `Bash(<command>)` so
    /// command wildcards like `Bash(git *)` work; other tools check by name.
    fn permission_key(tool_call: &ToolCall) -> String {
        match tool_call.name.as_str() {
            "bash" | "shell" => {
                let cmd = tool_call.arguments["command"].as_str().unwrap_or("");
                format!("Bash({})", cmd)
            }
            // self_manage's run_command action embeds a shell command and must
            // be gated like bash so Bash(...) deny rules apply to it.
            "self_manage" if tool_call.arguments["action"].as_str() == Some("run_command") => {
                let cmd = tool_call.arguments["command"].as_str().unwrap_or("");
                format!("Bash({})", cmd)
            }
            _ => tool_call.name.clone(),
        }
    }

    /// Gate a tool call: config rules first (deny beats every mode, explicit
    /// allow skips the mode check), then the session permission mode. `Ask`
    /// resolves to a refusal because the agent runs headless — the text is
    /// returned to the model as the tool result so it can adapt.
    fn check_permission(
        &self,
        rules: &super::permissions::PermissionRules,
        tool_call: &ToolCall,
    ) -> Result<(), String> {
        use super::permissions::PermissionAction;
        // Rules are checked against the permission key (Bash(<command>) for
        // shell tools) AND the raw tool name, so both `Bash(git *)` and plain
        // `shell`/`bash` rules work as users expect.
        let key = Self::permission_key(tool_call);
        let action = rules
            .check_explicit(&key)
            .or_else(|| {
                if key != tool_call.name {
                    rules.check_explicit(&tool_call.name)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                self.permission_mode.check_action(&tool_call.name, &tool_call.arguments)
            });
        match action {
            PermissionAction::Allow => Ok(()),
            PermissionAction::Deny(msg) => Err(format!("⛔ {} — the call was blocked.", msg)),
            PermissionAction::Ask(_) => Err(format!(
                "⛔ Tool '{}' requires interactive approval, which is not available on this channel. \
                 Explain to the user what you wanted to do instead.",
                tool_call.name
            )),
        }
    }

    /// Process a message through the agent loop — full featured
    pub async fn process(
        &self,
        message: &str,
        ctx: &mut FullContext,
        session: &mut Session,
    ) -> Result<String> {
        self.process_inner(message, ctx, session, None).await
    }

    /// Like `process`, but streams `AgentEvent`s (tokens, tool steps) to `tx`
    /// while the turn runs. Always ends with a `Done` or `Error` event. Send
    /// failures are ignored — a disconnected client must not poison the turn.
    pub async fn process_events(
        &self,
        message: &str,
        ctx: &mut FullContext,
        session: &mut Session,
        tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<String> {
        let result = self.process_inner(message, ctx, session, Some(&tx)).await;
        match &result {
            Ok(text) => { let _ = tx.send(AgentEvent::Done { text: text.clone() }); }
            Err(e) => { let _ = tx.send(AgentEvent::Error { message: e.to_string() }); }
        }
        result
    }

    async fn process_inner(
        &self,
        message: &str,
        ctx: &mut FullContext,
        session: &mut Session,
        events: Option<&tokio::sync::mpsc::UnboundedSender<AgentEvent>>,
    ) -> Result<String> {
        info!("Processing: {}", &message[..message.len().min(80)]);

        // Lifecycle hooks for this turn (operator only; None ⇒ no-op, no shell
        // runs — a remote-channel turn must never trigger the host's hooks).
        // Loaded off the async thread (sync fs read), like the facts injection.
        let hooks = {
            let operator = self.operator;
            let ws = self.workspace_dir.clone();
            tokio::task::spawn_blocking(move || Self::load_hooks_for(operator, &ws))
                .await
                .unwrap_or(None)
        };
        let first_turn = session.messages().is_empty();

        // Add daily note about this interaction
        let mem = super::memory::MemoryWorkspace::from_data_dir(&self.workspace_dir);
        let _ = mem.append_daily(&format!("User said: {}", &message[..message.len().min(200)]));

        // Add user message to session
        session.add_message(Message::user(message));

        // SessionStart (first turn of the session) + UserPromptSubmit hooks.
        if first_turn {
            Self::fire_hook(&hooks, super::hooks::HookEvent::SessionStart, self.hook_ctx(session, "session_start")).await;
        }
        {
            let mut hc = self.hook_ctx(session, "user_prompt_submit");
            hc.user_message = Some(message.to_string());
            Self::fire_hook(&hooks, super::hooks::HookEvent::UserPromptSubmit, hc).await;
        }

        // Learn from conversation
        ctx.observe(message);

        // Load the durable "what I know about you" facts off the async thread
        // (a sync SQLite open per turn would otherwise block the executor).
        // Best-effort: a missing/locked store yields no facts.
        let known_facts: Vec<String> = {
            let dir = self.workspace_dir.clone();
            tokio::task::spawn_blocking(move || {
                crate::memory::store::MemoryStore::open_in(&dir)
                    .and_then(|s| s.list_all(20))
                    .map(|facts| facts.into_iter().map(|f| f.value).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default()
        };

        // Standing orders: load off-thread (sync fs read), apply matched orders'
        // safe actions (context injection + note saves; RunCommand/Webhook
        // operator-gated inside apply_order_action).
        let orders = {
            let dd = self.workspace_dir.clone();
            tokio::task::spawn_blocking(move || super::standing_orders::StandingOrdersEngine::load(&dd))
                .await
                .unwrap_or_default()
        };
        let mut standing_context: Vec<String> = Vec::new();
        let msg_count = session.messages().len();
        for order in orders.matched(message, msg_count) {
            self.apply_order_action(&order, message, msg_count, &mem, &mut standing_context).await;
        }

        // Build full system prompt with persona + user model + memory + facts
        let system_prompt = self.build_system_prompt(ctx, &mem, &known_facts, &standing_context);

        // Build conversation messages
        let messages = self.build_messages(&system_prompt, session);

        // Call the LLM, then loop: execute the requested tool, feed the result
        // back, and let the model continue until it answers without a tool
        // call (or the iteration cap is hit). Skipped when tool dispatch is
        // disabled.
        let mut messages = messages;
        let mut response = self.call_llm_events(&messages, events).await?;

        let final_response = if self.tools_enabled {
            let config = crate::config::load().await?;
            let rules = super::permissions::PermissionRules {
                allow: config.permissions.allow.clone(),
                ask: config.permissions.ask.clone(),
                deny: config.permissions.deny.clone(),
            };
            let mut iterations = 0;
            while let Some(mut tool_call) = self.parse_tool_call(&response) {
                // A closed event sink means the streaming client is gone
                // (Stop pressed / tab closed). Abort the turn early so a
                // long multi-tool run can't hold the conversation lock —
                // and block other clients — for minutes after disconnect.
                if let Some(tx) = events {
                    if tx.is_closed() {
                        debug!("event sink closed — aborting turn early");
                        break;
                    }
                }
                if iterations >= MAX_TOOL_ITERATIONS {
                    response.push_str("\n\n[Stopped: tool iteration limit reached for this turn.]");
                    break;
                }
                iterations += 1;
                debug!("Tool round {}: {}", iterations, tool_call.name);

                if let Some(tx) = events {
                    let _ = tx.send(AgentEvent::ToolStart {
                        name: tool_call.name.clone(),
                        summary: Self::tool_summary(&tool_call),
                    });
                }
                // PreToolUse hooks may block the call or rewrite its arguments.
                let pre = {
                    let mut hc = self.hook_ctx(session, "pre_tool_use");
                    hc.tool_name = Some(tool_call.name.clone());
                    hc.tool_input = Some(tool_call.arguments.clone());
                    decide_pre_tool(
                        &Self::fire_hook(&hooks, super::hooks::HookEvent::PreToolUse, hc).await,
                    )
                };
                if let Some(modified) = pre.modified_input.clone() {
                    tool_call.arguments = modified;
                }

                let mut ok = true;
                let output = if pre.blocked {
                    ok = false;
                    format!("⛔ {}", pre.reason)
                } else {
                    match self.check_permission(&rules, &tool_call) {
                        Ok(()) => match self.execute_tool(&tool_call, ctx, session, &mem).await {
                            Ok(out) => out,
                            // Execution errors go back to the model as text so it
                            // can recover; only transport/config errors from
                            // call_llm abort the turn.
                            Err(e) => { ok = false; format!("Tool '{}' failed: {}", tool_call.name, e) }
                        },
                        Err(denial) => { ok = false; denial }
                    }
                };

                // PostToolUse / PostToolUseFailure hooks (observe the result).
                {
                    let mut hc = self.hook_ctx(
                        session,
                        if ok { "post_tool_use" } else { "post_tool_use_failure" },
                    );
                    hc.tool_name = Some(tool_call.name.clone());
                    hc.tool_output = Some(output.clone());
                    let ev = if ok {
                        super::hooks::HookEvent::PostToolUse
                    } else {
                        super::hooks::HookEvent::PostToolUseFailure
                    };
                    Self::fire_hook(&hooks, ev, hc).await;
                }

                let output = truncate_output(&output, MAX_TOOL_OUTPUT_BYTES);
                if let Some(tx) = events {
                    let _ = tx.send(AgentEvent::ToolEnd {
                        name: tool_call.name.clone(),
                        ok,
                        preview: truncate_output(&output, 200),
                    });
                }

                // Record the trajectory in both the working message list and
                // the session, so later turns keep the tool context.
                let result_msg = format!("[TOOL RESULT: {}]\n{}", tool_call.name, output);
                session.add_message(Message::assistant(&response));
                session.add_message(Message::user(&result_msg));
                messages.push(serde_json::json!({"role": "assistant", "content": response}));
                messages.push(serde_json::json!({"role": "user", "content": result_msg}));

                response = self.call_llm_events(&messages, events).await?;
            }
            response
        } else {
            response
        };

        // Add assistant response to session
        session.add_message(Message::assistant(&final_response));

        // Stop hook (end of turn).
        {
            let mut hc = self.hook_ctx(session, "stop");
            hc.assistant_message = Some(final_response.clone());
            Self::fire_hook(&hooks, super::hooks::HookEvent::Stop, hc).await;
        }

        // SessionEnd standing orders (e.g. the session-summary note).
        {
            let count = session.messages().len();
            for order in orders.check_session_end() {
                if let super::standing_orders::OrderAction::SaveNote { template } = &order.action {
                    let _ = mem.append_daily(&super::standing_orders::render_template(template, &final_response, count));
                }
            }
        }

        // Daily note about response
        let _ = mem.append_daily(&format!("Assistant responded: {}", &final_response[..final_response.len().min(200)]));

        Ok(final_response)
    }

    /// Build full system prompt with persona, user model, memory, and tool instructions
    fn build_system_prompt(
        &self,
        ctx: &FullContext,
        mem: &super::memory::MemoryWorkspace,
        known_facts: &[String],
        standing_context: &[String],
    ) -> String {
        let mut prompt = ctx.build_system_prompt();

        // Context injected by matched standing orders this turn.
        if !standing_context.is_empty() {
            prompt.push_str("# Standing context\n");
            for line in standing_context {
                prompt.push_str(&format!("- {}\n", line));
            }
            prompt.push('\n');
        }

        // Memory context
        let memory_ctx = mem.build_context();
        if !memory_ctx.is_empty() {
            prompt.push_str("# Memory\n");
            prompt.push_str(&memory_ctx);
            prompt.push('\n');
        }

        // Durable, per-item facts about the user (the "what I know about you"
        // store), loaded off-thread by the caller and injected so the agent
        // actually uses what it has remembered.
        if !known_facts.is_empty() {
            prompt.push_str("# What I know about you\n");
            for value in known_facts {
                prompt.push_str(&format!("- {}\n", value));
            }
            prompt.push('\n');
        }

        // Tool instructions — only advertised when tool dispatch is enabled.
        // Otherwise the model would emit [TOOL:...] syntax that `process()` drops,
        // leaking unexecuted tool markup into the visible reply.
        if self.tools_enabled {
            prompt.push_str("# Available Tools\n");
            prompt.push_str("Use [TOOL:name:{\"arg\":\"value\"}] to invoke tools:\n");
            for tool in &self.tools {
                prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
            }
            // MCP server tools (callable as mcp__<server>__<tool>).
            if let Some(mcp) = &self.mcp {
                for tool in mcp.list_all_tools() {
                    prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
                }
            }
            prompt.push('\n');
            push_str(&mut prompt, "After you emit a tool call, STOP — the tool result will be sent back to you as a [TOOL RESULT: name] message, and you can then continue or emit the next tool call. One tool call per message.");
            push_str(&mut prompt, "When you learn a durable fact about the user (a preference, an ongoing project, important personal context), save it with [TOOL:remember:{\"action\":\"add\",\"value\":\"...\"}] so you still know it next time.");

            // Self-management instructions
            push_str(&mut prompt, "# Self-Management");
            push_str(&mut prompt, "- You can request updates to your own skills and memory");
            push_str(&mut prompt, "- Use [TOOL:name:{\"action\":\"read_skill\",\"name\":\"...\"}] to read skills");
            push_str(&mut prompt, "- Use [TOOL:name:{\"action\":\"update_memory\",\"content\":\"...\"}] to update MEMORY.md");
            push_str(&mut prompt, "- Use [TOOL:name:{\"action\":\"create_skill\",\"name\":\"...\",\"content\":\"...\"}] to create new skills");
            push_str(&mut prompt, "- Use [TOOL:name:{\"action\":\"self_update\"}] to check for openAssistant updates");
            push_str(&mut prompt, "- Use [TOOL:name:{\"action\":\"run_command\",\"command\":\"...\"}] to run terminal commands (with user permission)");

            // Multi-agent goal deliberation instructions
            push_str(&mut prompt, "\n# Multi-Agent Goal Deliberation");
            push_str(&mut prompt, "When working on complex goals, use [TOOL:goal_deliberate:{\"title\":\"...\",\"description\":\"...\"}] to spawn a deliberation with:");
            push_str(&mut prompt, "  ⚖️ Judge — unbiased feasibility evaluation");
            push_str(&mut prompt, "  😈 Devil's Advocate — challenges assumptions");
            push_str(&mut prompt, "  🔍 Researcher — gathers context and best practices");
            push_str(&mut prompt, "  🔧 Executor — plans concrete implementation steps");
            push_str(&mut prompt, "  🧩 Synthesizer — combines all perspectives into final plan");
            push_str(&mut prompt, "Use [TOOL:todo_write:{\"todos\":[...]}] to track progress across turns.");
            push_str(&mut prompt, "Use [TOOL:plan_mode:{\"action\":\"toggle\"}] to switch between plan and execute modes.");
        }

        prompt
    }

    fn build_messages(&self, system_prompt: &str, session: &Session) -> Vec<serde_json::Value> {
        let mut messages = vec![];

        messages.push(serde_json::json!({
            "role": "system",
            "content": system_prompt
        }));

        // Last 30 messages for context
        for msg in session.messages().iter().rev().take(30).rev() {
            messages.push(serde_json::json!({
                "role": msg.role,
                "content": msg.content
            }));
        }

        messages
    }

    /// Resolve the `(api_base, api_key, model)` for the `text` modality.
    ///
    /// Multi-model routing is strictly opt-in: when `routing.text` names a real
    /// provider we use the resolved triple; otherwise we reproduce the legacy
    /// behavior exactly — the agent's own `self.model` against `config.model.*`
    /// creds. (Risk 5: routing must not silently override `Agent::new(model)`.)
    fn text_target<'a>(&'a self, config: &'a crate::config::Config) -> (&'a str, &'a str, &'a str) {
        let route = &config.routing.text;
        let routing_active = !route.provider.is_empty()
            && !route.model.is_empty()
            && config.providers.iter().any(|p| p.name == route.provider);
        if routing_active {
            crate::config::resolve_provider(config, "text")
        } else {
            (&config.model.api_base, &config.model.api_key, self.model.as_str())
        }
    }

    async fn call_llm(&self, messages: &[serde_json::Value]) -> Result<String> {
        let config = crate::config::load().await?;
        let client = reqwest::Client::new();
        let (api_base, api_key, model) = self.text_target(&config);
        call_llm_raw(&client, api_base, api_key, model, messages).await
    }

    /// LLM call that streams tokens to `tx` when an event sink is attached,
    /// otherwise behaves exactly like `call_llm`.
    async fn call_llm_events(
        &self,
        messages: &[serde_json::Value],
        events: Option<&tokio::sync::mpsc::UnboundedSender<AgentEvent>>,
    ) -> Result<String> {
        match events {
            Some(tx) => {
                let config = crate::config::load().await?;
                let client = reqwest::Client::new();
                let (api_base, api_key, model) = self.text_target(&config);
                call_llm_stream(&client, api_base, api_key, model, messages, tx).await
            }
            None => self.call_llm(messages).await,
        }
    }

    /// Execute a single parsed tool call and return ONLY the tool output —
    /// the agent loop feeds it back to the model as a [TOOL RESULT] message.
    async fn execute_tool(
        &self,
        tool_call: &ToolCall,
        ctx: &mut FullContext,
        session: &mut Session,
        mem: &super::memory::MemoryWorkspace,
    ) -> Result<String> {
        let _ = session; // kept in the signature for handlers that will need it
        debug!("Tool call: {}", tool_call.name);

        // Route to appropriate handler
        match tool_call.name.as_str() {
            // ── Claude Code core tools ──
            "bash" => {
                let result = crate::tools::bash::execute(&tool_call.arguments).await?;
                if result.exit_code == 0 && !result.timed_out {
                    Ok(format!("Bash output:\n{}", result.output))
                } else if result.timed_out {
                    Ok(format!("Bash timed out:\n{}", result.output))
                } else {
                    Ok(format!("Bash failed (exit {}):\n{}", result.exit_code, result.output))
                }
            }
            "read" => {
                let result = crate::tools::file::execute(&serde_json::json!({
                    "action": "read",
                    "path": tool_call.arguments["path"].as_str().unwrap_or("")
                })).await?;
                Ok(format!("File content:\n{}", result.output))
            }
            "write" => {
                let result = crate::tools::file::execute(&serde_json::json!({
                    "action": "write",
                    "path": tool_call.arguments["path"].as_str().unwrap_or(""),
                    "content": tool_call.arguments["content"].as_str().unwrap_or("")
                })).await?;
                Ok(format!("Write result:\n{}", result.output))
            }
            "edit" => {
                let path = tool_call.arguments["path"].as_str().unwrap_or("");
                let old = tool_call.arguments["old_string"].as_str().unwrap_or("");
                let new = tool_call.arguments["new_string"].as_str().unwrap_or("");
                // Read file, replace, write back
                let read_result = crate::tools::file::execute(&serde_json::json!({
                    "action": "read", "path": path
                })).await?;
                if read_result.success {
                    let content = &read_result.output;
                    if content.contains(old) {
                        let replaced = content.replace(old, new);
                        let write_result = crate::tools::file::execute(&serde_json::json!({
                            "action": "write", "path": path, "content": replaced
                        })).await?;
                        Ok(format!("Edit result: {}", write_result.output))
                    } else {
                        Ok(format!("Edit error: old_string not found in {}", path))
                    }
                } else {
                    Ok(format!("Edit error: could not read {}", path))
                }
            }
            "glob" => {
                let result = crate::tools::file_search::glob(&tool_call.arguments).await?;
                let files: Vec<String> = result.files.clone();
                Ok(format!("Glob results ({} files):\n{}", result.total_found, files.join("\n")))
            }
            "grep" => {
                let result = crate::tools::file_search::grep(&tool_call.arguments).await?;
                let mut output = format!("Grep results ({} matches in {} files):\n", result.total_matches, result.files_searched);
                for m in &result.matches {
                    output.push_str(&format!("  {}:{}: {}\n", m.file, m.line_number, m.line));
                }
                Ok(output)
            }
            "todo_write" => {
                self.handle_todo_write(&tool_call.arguments).await
            }
            "goal_deliberate" => {
                self.handle_goal_deliberate(&tool_call.arguments).await
            }
            "claude" => self.handle_claude(&tool_call.arguments).await,
            "goal_create" => self.handle_goal_create(&tool_call.arguments).await,
            "goal_subgoal" => self.handle_goal_subgoal(&tool_call.arguments).await,
            "goal_task" => self.handle_goal_task(&tool_call.arguments).await,
            "goal_list" => self.handle_goal_list().await,
            "task" => {
                self.handle_task_tool(&tool_call.arguments).await
            }
            "plan_mode" => {
                self.handle_plan_mode(&tool_call.arguments).await
            }
            "perm" => {
                self.handle_perm(&tool_call.arguments).await
            }
            "web_search" => {
                let query = tool_call.arguments["query"].as_str().unwrap_or("");
                if query.is_empty() {
                    return Ok("web_search: missing 'query' argument.".to_string());
                }
                let engine = tool_call.arguments["engine"].as_str().unwrap_or("duckduckgo");
                let ws = crate::core::web_search::WebSearch::default();
                match ws.search_with(engine, query).await {
                    Ok(results) if results.is_empty() => {
                        Ok(format!("Web search ({}): no results for '{}'.", engine, query))
                    }
                    Ok(results) => {
                        let mut out = format!("Web search results for '{}':\n", query);
                        for r in results.iter().take(5) {
                            out.push_str(&format!("- {} — {}\n  {}\n", r.title, r.url, r.snippet));
                        }
                        Ok(out)
                    }
                    Err(e) => Ok(format!("Web search failed ({}): {}", engine, e)),
                }
            }
            // ── Legacy openAssistant tools ──
            "shell" => {
                let result = crate::tools::shell::execute(&tool_call.arguments).await?;
                Ok(format!("Shell output:\n{}", result.output))
            }
            "file" => {
                let result = crate::tools::file::execute(&tool_call.arguments).await?;
                Ok(format!("File result:\n{}", result.output))
            }
            "browser" => {
                let result = crate::tools::browser::execute(&tool_call.arguments).await?;
                Ok(format!("Browser result:\n{}", result.output))
            }
            "vision" => {
                let result = crate::tools::vision::execute(&tool_call.arguments).await?;
                Ok(format!("Vision result:\n{}", result.output))
            }
            "watch" => self.handle_watch(&tool_call.arguments).await,
            "remember" => self.handle_remember(&tool_call.arguments).await,
            "memory" => {
                self.handle_memory_tool(&tool_call.arguments, mem).await
            }
            "self_manage" => {
                self.handle_self_manage(&tool_call.arguments, mem, ctx).await
            }
            name if name.starts_with("mcp__") => match &self.mcp {
                Some(reg) => match reg.call_prefixed(name, tool_call.arguments.clone()).await {
                    Ok(out) => Ok(out),
                    Err(e) => Ok(format!("MCP error: {}", e)),
                },
                None => Ok("No MCP servers are configured (add them to <data_dir>/.mcp.json).".to_string()),
            },
            _ => {
                tracing::warn!("Unknown tool: {}", tool_call.name);
                Ok(format!("Unknown tool '{}'. Available tools are listed in the system prompt.", tool_call.name))
            }
        }
    }

    // ── Claude Code tool handlers ──

    async fn handle_todo_write(&self, args: &serde_json::Value) -> Result<String> {
        let todos = args["todos"].as_array().cloned().unwrap_or_default();
        let mut items = Vec::new();
        for todo in &todos {
            let content = todo["content"].as_str().unwrap_or("").to_string();
            let status = todo["status"].as_str().unwrap_or("pending").to_string();
            let priority = todo["priority"].as_str().unwrap_or("medium").to_string();
            items.push((content, status, priority));
        }

        let mut todo_list = super::goal_system::TodoList::new();
        let ids = todo_list.replace(items);
        Ok(format!("✅ Todo list updated with {} items:\n{}", ids.len(), todo_list.format_todos()))
    }

    async fn handle_goal_deliberate(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("Untitled Goal");
        let description = args["description"].as_str().unwrap_or("");

        let mut deliberator = super::goal_system::GoalDeliberator::new();
        let goal_id = format!("goal_{}", uuid::Uuid::new_v4());
        deliberator.start_deliberation(&goal_id, title, description);

        // Each deliberation role makes a real LLM call with its role prompt and
        // the accumulated context of prior contributions.
        let config = crate::config::load().await?;
        let client = reqwest::Client::new();
        let (api_base, api_key, model) = self.text_target(&config);

        let roles = deliberator.default_roles().to_vec();
        for role in &roles {
            let Some(prompt) = deliberator.build_role_prompt(&goal_id, role) else { continue };
            let messages = vec![serde_json::json!({ "role": "user", "content": prompt })];
            let content = match call_llm_raw(&client, api_base, api_key, model, &messages).await {
                Ok(c) if !c.trim().is_empty() => c,
                Ok(_) => format!("[{} returned no content]", role.name()),
                Err(e) => format!("[{} failed: {}]", role.name(), e),
            };
            let is_synth = matches!(role, super::goal_system::DeliberationRole::Synthesizer);
            deliberator.add_contribution(&goal_id, role.clone(), &content, Some(0.8));
            if is_synth {
                deliberator.set_synthesis(&goal_id, &content);
            }
        }

        // Persist the deliberated goal so it survives across runs.
        if let Ok(mut store) = super::goal_store::GoalStore::open_default() {
            if let Err(e) = store.create_goal(title, description) {
                tracing::warn!("Could not persist deliberated goal: {}", e);
            }
        }

        Ok(deliberator.format_deliberation(&goal_id))
    }

    // ── Claude Code CLI bridge ──

    async fn handle_claude(&self, args: &serde_json::Value) -> Result<String> {
        let prompt = args["prompt"].as_str().unwrap_or("").trim();
        if prompt.is_empty() {
            return Ok("Provide a 'prompt' for the claude tool.".to_string());
        }
        let resume = args["resume"].as_str();
        let config = crate::config::load().await?;
        // LLM tool-loop calls are REMOTE origin (no `.operator()`): the bridge
        // refuses --dangerously-skip-permissions and caps the permission mode.
        let bridge = super::claude_bridge::ClaudeBridge::from_config(&config.claude, &self.workspace_dir);
        match bridge.run(prompt, resume).await {
            Ok(r) => Ok(r.text),
            Err(e) => Ok(format!("Claude bridge error: {}", e)),
        }
    }

    // ── Goal / subgoal tools (persisted via GoalStore) ──

    async fn handle_goal_create(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("Untitled Goal");
        let description = args["description"].as_str().unwrap_or("");
        let mut store = super::goal_store::GoalStore::open_default()?;
        let id = store.create_goal(title, description)?;
        Ok(format!("🎯 Created goal '{}' [{}]", title, &id[..id.len().min(8)]))
    }

    async fn handle_goal_subgoal(&self, args: &serde_json::Value) -> Result<String> {
        let goal = args["goal"].as_str().unwrap_or("");
        let title = args["title"].as_str().unwrap_or("Untitled Subgoal");
        let description = args["description"].as_str().unwrap_or("");
        let mut store = super::goal_store::GoalStore::open_default()?;
        match store.resolve_goal_id(goal) {
            Some(gid) => match store.add_subgoal(&gid, title, description)? {
                Some(sid) => Ok(format!(
                    "➕ Added subgoal '{}' [{}] to goal [{}]",
                    title, &sid[..sid.len().min(8)], &gid[..gid.len().min(8)]
                )),
                None => Ok("Failed to add subgoal.".to_string()),
            },
            None => Ok(format!("No goal matching '{}'. Use goal_list to see goals.", goal)),
        }
    }

    async fn handle_goal_task(&self, args: &serde_json::Value) -> Result<String> {
        let goal = args["goal"].as_str().unwrap_or("");
        let subgoal = args["subgoal"].as_str().unwrap_or("");
        let title = args["title"].as_str().unwrap_or("Untitled Task");
        let mut store = super::goal_store::GoalStore::open_default()?;
        let gid = match store.resolve_goal_id(goal) {
            Some(g) => g,
            None => return Ok(format!("No goal matching '{}'.", goal)),
        };
        let sgid = store.board.get_goal(&gid).and_then(|g| {
            g.subgoals
                .iter()
                .find(|s| s.id == subgoal || s.id.starts_with(subgoal) || s.title.eq_ignore_ascii_case(subgoal))
                .map(|s| s.id.clone())
        });
        match sgid {
            Some(sid) => match store.add_task(&gid, &sid, title)? {
                Some(tid) => Ok(format!(
                    "✅ Added task '{}' [{}] under subgoal [{}]",
                    title, &tid[..tid.len().min(8)], &sid[..sid.len().min(8)]
                )),
                None => Ok("Failed to add task.".to_string()),
            },
            None => Ok(format!("No subgoal matching '{}' in goal [{}].", subgoal, &gid[..gid.len().min(8)])),
        }
    }

    async fn handle_goal_list(&self) -> Result<String> {
        let store = super::goal_store::GoalStore::open_default()?;
        Ok(store.format())
    }

    /// Manage URL watchers (persisted in <data_dir>/proactive.json; checked by
    /// the gateway's proactive loop, which posts change notifications).
    async fn handle_watch(&self, args: &serde_json::Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("list");
        let mut store = super::watchers::WatcherStore::open(&self.workspace_dir);
        match action {
            "add" => {
                let url = args["url"].as_str().unwrap_or("").trim();
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Ok("watch add: provide an http(s) 'url'.".to_string());
                }
                let note = args["note"].as_str().unwrap_or("");
                let interval = args["interval_minutes"].as_u64().unwrap_or(60);
                let id = store.add(url, note, interval)?;
                Ok(format!(
                    "🔭 Watching {} every {}m [{}]. I'll post here when it changes (gateway must be running).",
                    url,
                    interval.max(super::watchers::MIN_INTERVAL_MINUTES),
                    &id[..8]
                ))
            }
            "remove" => {
                let key = args["url"].as_str().or_else(|| args["id"].as_str()).unwrap_or("");
                if key.is_empty() {
                    return Ok("watch remove: provide 'url' or 'id'.".to_string());
                }
                if store.remove(key)? {
                    Ok(format!("Stopped watching {}.", key))
                } else {
                    Ok(format!("No watcher matching '{}'. Use action=list.", key))
                }
            }
            _ => Ok(store.format_list()),
        }
    }

    /// Load lifecycle hooks for this turn — only for the trusted local
    /// operator. Returns `None` for remote/gateway agents (so `fire_hook` is a
    /// no-op and no shell runs). A missing hooks.json yields an empty engine.
    /// Associated fn (owned args) so it can run inside `spawn_blocking`.
    fn load_hooks_for(operator: bool, workspace_dir: &str) -> Option<super::hooks::HookEngine> {
        if !operator {
            return None;
        }
        super::hooks::HookEngine::load_from_workspace(workspace_dir).ok()
    }

    /// Build a `HookContext` for `session`/`event`; callers set the
    /// tool/message fields they have.
    fn hook_ctx(&self, session: &Session, event: &str) -> super::hooks::HookContext {
        super::hooks::HookContext {
            session_id: session.id.clone(),
            workspace_dir: self.workspace_dir.clone(),
            event: event.to_string(),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            user_message: None,
            assistant_message: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Fire an event if hooks are loaded (operator only); else a no-op.
    async fn fire_hook(
        engine: &Option<super::hooks::HookEngine>,
        event: super::hooks::HookEvent,
        ctx: super::hooks::HookContext,
    ) -> Vec<super::hooks::HookResult> {
        match engine {
            Some(e) => e.fire(&event, &ctx).await,
            None => Vec::new(),
        }
    }

    /// Apply one matched standing order's action. Safe actions (InjectContext,
    /// SaveNote) run on any origin; RunCommand/Webhook run only for the local
    /// operator; re-entrant actions are logged no-ops in v1.
    async fn apply_order_action(
        &self,
        order: &super::standing_orders::StandingOrder,
        message: &str,
        message_count: usize,
        mem: &super::memory::MemoryWorkspace,
        ctx_out: &mut Vec<String>,
    ) {
        use super::standing_orders::{render_template, OrderAction};
        match &order.action {
            OrderAction::InjectContext { text } => ctx_out.push(text.clone()),
            OrderAction::SaveNote { template } => {
                // Safe on any origin: the agent already logs every turn to the
                // daily note, so a remote user's text is no new exposure. (The
                // injected context for EveryNMessages uses a fixed `text` the
                // remote user does not control.)
                let _ = mem.append_daily(&render_template(template, message, message_count));
            }
            OrderAction::RunCommand { command } => {
                if !self.operator {
                    tracing::warn!("standing order '{}' RunCommand skipped (remote origin)", order.name);
                    return;
                }
                let (shell, flag) = super::hooks::hook_shell();
                // Bounded like hooks: a hung command must not freeze the turn.
                let run = tokio::process::Command::new(shell).arg(flag).arg(command).output();
                match tokio::time::timeout(std::time::Duration::from_secs(30), run).await {
                    Ok(Ok(o)) => info!("standing order '{}' ran (exit {:?})", order.name, o.status.code()),
                    Ok(Err(e)) => tracing::warn!("standing order '{}' command failed: {}", order.name, e),
                    Err(_) => tracing::warn!("standing order '{}' RunCommand timed out (30s)", order.name),
                }
            }
            OrderAction::Webhook { url, body } => {
                if !self.operator {
                    tracing::warn!("standing order '{}' Webhook skipped (remote origin)", order.name);
                    return;
                }
                let client = reqwest::Client::new();
                if let Err(e) = client.post(url).body(body.clone()).send().await {
                    tracing::warn!("standing order '{}' webhook failed: {}", order.name, e);
                }
            }
            other => {
                debug!("standing order '{}' action {:?} not auto-executed in v1", order.name, std::mem::discriminant(other));
            }
        }
    }

    /// Durable, per-item facts about the user (the "what I know about you"
    /// store, surfaced in the desktop Memory panel and injected into the system
    /// prompt). Persisted in `<data_dir>/memory.db` via `MemoryStore`.
    async fn handle_remember(&self, args: &serde_json::Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("add");
        let store = match crate::memory::store::MemoryStore::open_in(&self.workspace_dir) {
            Ok(s) => s,
            Err(e) => return Ok(format!("remember: could not open memory store: {}", e)),
        };
        match action {
            "add" => {
                let value = args["value"].as_str().unwrap_or("").trim();
                if value.is_empty() {
                    return Ok("remember add: provide a 'value' (the fact to remember).".to_string());
                }
                let key = match args["key"].as_str() {
                    Some(k) if !k.trim().is_empty() => k.trim().to_string(),
                    _ => derive_fact_key(value),
                };
                let category = args["category"].as_str().unwrap_or("fact");
                let importance = args["importance"].as_f64().unwrap_or(0.6);
                let entry = crate::memory::store::MemoryEntry::new(
                    key.clone(),
                    value,
                    category,
                    "agent",
                    importance,
                );
                match store.store(&entry) {
                    Ok(_) => Ok(format!("🧠 Remembered [{}]: {}", key, value)),
                    Err(e) => Ok(format!("remember: could not save: {}", e)),
                }
            }
            "list" => match store.list_all(50) {
                Ok(facts) if facts.is_empty() => Ok("I don't have any saved facts yet.".to_string()),
                Ok(facts) => {
                    let mut out = String::from("What I remember about you (forget by id or key):\n");
                    for f in &facts {
                        out.push_str(&format!(
                            "- [id={} key={}] {} ({:.1})\n",
                            f.id.unwrap_or(0), f.key, f.value, f.importance
                        ));
                    }
                    Ok(out)
                }
                Err(e) => Ok(format!("remember list: {}", e)),
            },
            "forget" => {
                // Prefer a precise id (from `list`); fall back to key. Note that
                // key-based delete removes every fact sharing that derived key.
                if let Some(id) = args["id"].as_i64() {
                    return Ok(match store.delete_by_id(id) {
                        Ok(0) => format!("No saved fact with id {}.", id),
                        Ok(_) => format!("Forgot fact [id={}].", id),
                        Err(e) => format!("remember forget: {}", e),
                    });
                }
                let key = args["key"].as_str().unwrap_or("").trim();
                if key.is_empty() {
                    return Ok("remember forget: provide an 'id' or 'key' to forget (see action=list).".to_string());
                }
                match store.delete(key) {
                    Ok(0) => Ok(format!("No saved fact with key '{}'.", key)),
                    Ok(n) => Ok(format!("Forgot {} fact(s) with key '{}'.", n, key)),
                    Err(e) => Ok(format!("remember forget: {}", e)),
                }
            }
            _ => Ok(format!("Unknown remember action '{}'. Use add | list | forget.", action)),
        }
    }

    /// One-line human summary of a tool call for the streaming tool timeline.
    fn tool_summary(tool_call: &ToolCall) -> String {
        let args = &tool_call.arguments;
        let s = match tool_call.name.as_str() {
            "bash" | "shell" => args["command"].as_str().unwrap_or(""),
            "read" | "write" | "edit" => args["path"].as_str().unwrap_or(""),
            "glob" | "grep" => args["pattern"].as_str().unwrap_or(""),
            "web_search" => args["query"].as_str().unwrap_or(""),
            "task" => args["description"].as_str().unwrap_or(""),
            _ => "",
        };
        if s.is_empty() {
            truncate_output(&args.to_string(), 80)
        } else {
            truncate_output(s, 80)
        }
    }

    /// Restrict the default tool set to an allowlist (sub-agent tool scoping).
    fn filtered_tools(allowed: &[String]) -> Vec<ToolDefinition> {
        let mut tools: Vec<ToolDefinition> = Self::default_tools()
            .into_iter()
            .filter(|t| allowed.iter().any(|a| a == &t.name))
            .collect();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    async fn handle_task_tool(&self, args: &serde_json::Value) -> Result<String> {
        if self.depth >= 1 {
            return Ok("Sub-agent refused: nested sub-agents are not allowed (depth limit 1).".to_string());
        }
        let subagent_type = args["subagent_type"].as_str().unwrap_or("General");
        let description = args["description"].as_str().unwrap_or("");
        let prompt = args["prompt"].as_str().unwrap_or("");
        if prompt.is_empty() {
            return Ok("task: missing 'prompt' argument.".to_string());
        }

        let tools: Vec<String> = args["tools"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
            .unwrap_or_else(|| vec!["read".to_string(), "glob".to_string(), "grep".to_string()]);

        let role_desc = match subagent_type {
            "Explore" => "fast agent specialized for exploring codebases: finding files, searching code, understanding project structure",
            "Plan" => "agent specialized for planning and design: implementation plans, architectures, breaking down complex tasks",
            _ => "general-purpose agent for multi-step work that requires reasoning and tool use",
        };

        let mut child = Agent::new(self.model.clone())
            .with_workspace(self.workspace_dir.clone())
            .with_permission_mode(self.permission_mode);
        child.depth = self.depth + 1;
        child.tools = Self::filtered_tools(&tools);

        let mut child_ctx = super::persona::FullContext::new();
        let mut child_session = super::session::Session::new("subagent", "local");
        let task_prompt = format!(
            "You are a {role_desc}. Complete this task and report your findings as your final message.\n\
             Task: {description}\n\n{prompt}"
        );

        // Box the recursive async call (process → execute_tool → here).
        let result = Box::pin(child.process(&task_prompt, &mut child_ctx, &mut child_session)).await;
        match result {
            Ok(text) => Ok(format!("🤖 Sub-agent ({}) result:\n{}", subagent_type, text)),
            Err(e) => Ok(format!("Sub-agent ({}) failed: {}", subagent_type, e)),
        }
    }

    async fn handle_plan_mode(&self, args: &serde_json::Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("status");
        match action {
            "toggle" => Ok("📋 Plan mode toggled. Use [TOOL:plan_mode:{\"action\":\"status\"}] to check current mode.".to_string()),
            "on" => Ok("📋 Plan mode ON — read-only mode activated. You can explore and plan but not modify files.".to_string()),
            "off" => Ok("⚡ Plan mode OFF — execute mode activated. You can now read, write, and execute.".to_string()),
            _ => Ok("Current mode: Execute (read-write). Use [TOOL:plan_mode:{\"action\":\"on\"}] to enter plan mode.".to_string()),
        }
    }

    async fn handle_perm(&self, args: &serde_json::Value) -> Result<String> {
        let tool = args["tool"].as_str().unwrap_or("");
        let action = args["action"].as_str().unwrap_or("status");

        if tool.is_empty() {
            let mgr = super::goal_system::PermissionManager::new();
            return Ok(mgr.format_permissions());
        }

        let perm = super::goal_system::Permission::from_str(action);
        Ok(format!("🔐 Permission for '{}': {:?} ({})\nIn a full implementation, this would persist to config.", tool, perm, action))
    }

    // ── Legacy tool handlers ──

    async fn handle_memory_tool(
        &self,
        args: &serde_json::Value,
        mem: &super::memory::MemoryWorkspace,
    ) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("search");

        match action {
            "search" => {
                let query = args["query"].as_str().unwrap_or("");
                let results = mem.search_files(query);
                if results.is_empty() {
                    Ok("No memory found for that query.".to_string())
                } else {
                    let formatted: Vec<String> = results
                        .iter()
                        .map(|(file, line)| format!("[{}] {}", file, line))
                        .collect();
                    Ok(format!("Memory search results:\n{}", formatted.join("\n")))
                }
            }
            "store" => {
                let content = args["content"].as_str().unwrap_or("");
                mem.append_long_term(content)?;
                mem.append_daily(&format!("Stored to memory: {}", &content[..content.len().min(100)]))?;
                Ok("Stored in long-term memory (MEMORY.md) and daily notes.".to_string())
            }
            "read" => {
                let lt = mem.read_long_term();
                let today = mem.read_today();
                Ok(format!("--- MEMORY.md ---\n{}\n\n--- Today ---\n{}", lt, today))
            }
            _ => Ok(format!("Unknown memory action: {}", action)),
        }
    }

    async fn handle_self_manage(
        &self,
        args: &serde_json::Value,
        mem: &super::memory::MemoryWorkspace,
        ctx: &mut FullContext,
    ) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("");

        match action {
            "read_skill" => {
                let name = args["name"].as_str().unwrap_or("");
                let path = format!("{}/skills/{}.md", self.workspace_dir, name);
                let content = tokio::fs::read_to_string(&path).await.unwrap_or_else(|_| format!("Skill '{}' not found", name));
                Ok(content)
            }
            "create_skill" => {
                let name = args["name"].as_str().unwrap_or("unnamed");
                let content = args["content"].as_str().unwrap_or("");
                tokio::fs::create_dir_all(format!("{}/skills", self.workspace_dir)).await?;
                let path = format!("{}/skills/{}.md", self.workspace_dir, name);
                tokio::fs::write(&path, content).await?;
                Ok(format!("Created skill '{}.md' in workspace/skills/", name))
            }
            "update_memory" => {
                let content = args["content"].as_str().unwrap_or("");
                mem.write_long_term(content)?;
                Ok("MEMORY.md updated.".to_string())
            }
            "update_memory_append" => {
                let content = args["content"].as_str().unwrap_or("");
                mem.append_long_term(content)?;
                Ok("Appended to MEMORY.md.".to_string())
            }
            "list_skills" => {
                let skills_dir = format!("{}/skills", self.workspace_dir);
                let mut skills = Vec::new();
                if let Ok(mut entries) = tokio::fs::read_dir(&skills_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        if let Some(name) = entry.file_name().to_str() {
                            skills.push(name.to_string());
                        }
                    }
                }
                Ok(format!("Available skills:\n- {}", skills.join("\n- ")))
            }
            "run_command" => {
                let command = args["command"].as_str().unwrap_or("");
                if command.is_empty() {
                    return Ok("No command provided".to_string());
                }
                let result = crate::tools::shell::execute(&serde_json::json!({"command": command})).await?;
                Ok(format!("Command output:\n{}", result.output))
            }
            "self_update" => {
                Ok("To see pending updates run `openassistant update --check`; to apply them run `openassistant update` (git-based, source checkouts only).".to_string())
            }
            "set_persona" => {
                let key = args["key"].as_str().unwrap_or("");
                let value = args["value"].as_str().unwrap_or("");
                match key {
                    "name" => { ctx.persona.name = value.to_string(); }
                    "tone" => { ctx.persona.tone = value.to_string(); }
                    "emoji" => { ctx.persona.emoji = value.to_string(); }
                    "personality" => { ctx.persona.personality = value.to_string(); }
                    _ => return Ok(format!("Unknown persona key: {}. Valid: name, tone, emoji, personality", key)),
                }
                Ok(format!("Updated persona.{} = {}", key, value))
            }
            "set_user" => {
                let key = args["key"].as_str().unwrap_or("");
                let value = args["value"].as_str().unwrap_or("");
                match key {
                    "name" => { ctx.user.name = value.to_string(); }
                    "what_to_call_them" => { ctx.user.what_to_call_them = value.to_string(); }
                    "technical_level" => { ctx.user.technical_level = value.to_string(); }
                    "interests" => { ctx.user.interests.push(value.to_string()); }
                    "projects" => { ctx.user.projects.push(value.to_string()); }
                    _ => return Ok(format!("Unknown user key: {}. Valid: name, what_to_call_them, technical_level, interests, projects", key)),
                }
                Ok(format!("Updated user.{} = {}", key, value))
            }
            _ => Ok(format!("Unknown self_manage action: {}", action)),
        }
    }

    fn parse_tool_call(&self, text: &str) -> Option<ToolCall> {
        let re = regex::Regex::new(r"\[TOOL:(\w+):(\{.*?\})\]").ok()?;
        re.captures(text).map(|caps| ToolCall {
            name: caps[1].to_string(),
            arguments: serde_json::from_str(&caps[2]).unwrap_or(serde_json::json!({})),
        })
    }

    fn default_tools() -> Vec<ToolDefinition> {
        vec![
            // ── Claude Code core tools ──
            ToolDefinition {
                name: "bash".to_string(),
                description: "Execute shell command with timeout. Args: {\"command\": \"...\", \"timeout_ms\": 120000, \"working_dir\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}, "timeout_ms": {"type": "integer"}, "working_dir": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "glob".to_string(),
                description: "Find files by glob pattern. Args: {\"pattern\": \"*.rs\", \"path\": \".\", \"max_results\": 100}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}, "max_results": {"type": "integer"}}}),
            },
            ToolDefinition {
                name: "grep".to_string(),
                description: "Search file contents with regex. Args: {\"pattern\": \"fn main\", \"path\": \".\", \"glob\": \"*.rs\", \"max_results\": 50}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}, "glob": {"type": "string"}, "max_results": {"type": "integer"}}}),
            },
            ToolDefinition {
                name: "read".to_string(),
                description: "Read file contents. Args: {\"path\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "write".to_string(),
                description: "Write content to a file. Args: {\"path\": \"...\", \"content\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "edit".to_string(),
                description: "Edit file by replacing exact text. Args: {\"path\": \"...\", \"old_string\": \"...\", \"new_string\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}, "old_string": {"type": "string"}, "new_string": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "todo_write".to_string(),
                description: "Replace the entire todo list. Args: {\"todos\": [{\"content\": \"...\", \"status\": \"pending|in_progress|completed\", \"priority\": \"high|medium|low\"}]}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"todos": {"type": "array", "items": {"type": "object", "properties": {"content": {"type": "string"}, "status": {"type": "string"}, "priority": {"type": "string"}}}}}}),
            },
            ToolDefinition {
                name: "goal_deliberate".to_string(),
                description: "Start multi-agent deliberation. Spawns Judge, Devil's Advocate, Researcher, Executor, Synthesizer. Args: {\"title\": \"...\", \"description\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}, "description": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "task".to_string(),
                description: "Spawn sub-agent. Args: {\"subagent_type\": \"Explore|Plan|General\", \"description\": \"...\", \"prompt\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"subagent_type": {"type": "string"}, "description": {"type": "string"}, "prompt": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "claude".to_string(),
                description: "Delegate a task to the local Claude Code CLI (agentic coding/file/automation work on the configured project). Args: {\"prompt\": \"...\", \"resume\": \"<session-id, optional>\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"prompt": {"type": "string"}, "resume": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "goal_create".to_string(),
                description: "Create a persisted goal. Args: {\"title\": \"...\", \"description\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}, "description": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "goal_subgoal".to_string(),
                description: "Add a subgoal to a goal (by id-prefix or title). Args: {\"goal\": \"...\", \"title\": \"...\", \"description\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"goal": {"type": "string"}, "title": {"type": "string"}, "description": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "goal_task".to_string(),
                description: "Add a task under a subgoal. Args: {\"goal\": \"...\", \"subgoal\": \"...\", \"title\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"goal": {"type": "string"}, "subgoal": {"type": "string"}, "title": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "goal_list".to_string(),
                description: "List persisted goals with subgoal progress. Args: {}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            },
            ToolDefinition {
                name: "plan_mode".to_string(),
                description: "Toggle plan mode. Args: {\"action\": \"toggle|on|off|status\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"action": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "perm".to_string(),
                description: "Manage tool permissions. Args: {\"tool\": \"bash\", \"action\": \"allow|deny|ask|status\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"tool": {"type": "string"}, "action": {"type": "string"}}}),
            },
            // ── openAssistant tools ──
            ToolDefinition {
                name: "browser".to_string(),
                description: "Search or browse. Args: {\"action\": \"search|browse\", \"query\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"action": {"type": "string"}, "query": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "vision".to_string(),
                description: "Analyze image via Gemini CLI. Args: {\"image_path\": \"...\", \"question\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"image_path": {"type": "string"}, "question": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "memory".to_string(),
                description: "Search/store/read memory. Args: {\"action\": \"search|store|read\", \"query\": \"...\", \"content\": \"...\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"action": {"type": "string"}, "query": {"type": "string"}, "content": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "self_manage".to_string(),
                description: "Self-management. Args: {\"action\": \"read_skill|create_skill|update_memory|list_skills|run_command|self_update|set_persona|set_user\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"action": {"type": "string"}}}),
            },
            ToolDefinition {
                name: "watch".to_string(),
                description: "Watch a URL and get notified when it changes. Args: {\"action\": \"add|list|remove\", \"url\": \"https://...\", \"note\": \"why\", \"interval_minutes\": 60}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"action": {"type": "string"}, "url": {"type": "string"}, "note": {"type": "string"}, "interval_minutes": {"type": "integer"}}}),
            },
            ToolDefinition {
                name: "remember".to_string(),
                description: "Save, list, or forget a durable fact about the user (persists across restarts; shown in their memory panel). Args: {\"action\": \"add|list|forget\", \"value\": \"the fact\", \"key\": \"<optional handle>\", \"id\": <from list, for precise forget>, \"category\": \"fact|preference\", \"importance\": 0.6}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"action": {"type": "string"}, "value": {"type": "string"}, "key": {"type": "string"}, "id": {"type": "integer"}, "category": {"type": "string"}, "importance": {"type": "number"}}}),
            },
            ToolDefinition {
                name: "web_search".to_string(),
                description: "Multi-source web search. Args: {\"query\": \"...\", \"engine\": \"duckduckgo|brave|google\"}".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}, "engine": {"type": "string"}}}),
            },
        ]
    }
}

fn push_str(buf: &mut String, s: &str) {
    buf.push_str(s);
    buf.push('\n');
}

/// Make a single chat-completions call to an OpenAI-compatible endpoint.
///
/// This is the shared LLM primitive reused by the agent loop, the workflow
/// engine, and goal deliberation — each passes its own (shared) `reqwest::Client`
/// and the resolved provider credentials/model. Surfaces transport/HTTP errors
/// instead of silently returning an empty string.
use crate::memory::store::fact_key as derive_fact_key;

/// Outcome of the PreToolUse hooks for one tool call.
#[derive(Debug, Default, PartialEq)]
struct PreToolDecision {
    blocked: bool,
    /// Hook stderr/explanation when blocked.
    reason: String,
    /// Rewritten tool arguments (applied only when not blocked).
    modified_input: Option<serde_json::Value>,
}

/// Reduce PreToolUse hook results to a single decision: any `block` wins
/// (and short-circuits modify); otherwise the last `modified_input` applies.
fn decide_pre_tool(results: &[super::hooks::HookResult]) -> PreToolDecision {
    let mut decision = PreToolDecision::default();
    for r in results {
        if r.block {
            decision.blocked = true;
            // Prefer the hook's structured reason, then stderr, then a generic.
            decision.reason = r
                .block_reason
                .clone()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| {
                    let e = r.stderr.trim();
                    (!e.is_empty()).then(|| e.to_string())
                })
                .unwrap_or_else(|| "blocked by a PreToolUse hook".to_string());
            decision.modified_input = None;
            return decision;
        }
        if r.modified_input.is_some() {
            decision.modified_input = r.modified_input.clone();
        }
    }
    decision
}

/// Cap tool output before feeding it back into the context window.
fn truncate_output(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…[truncated {} bytes]", &s[..end], s.len() - end)
}

pub(crate) async fn call_llm_raw(
    client: &reqwest::Client,
    api_base: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
) -> Result<String> {
    if api_key.trim().is_empty() {
        anyhow::bail!(
            "No API key configured for model '{}'. Set model.api_key (or the routed provider's key) first.",
            model
        );
    }

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": 0.7,
        "max_tokens": 8192,
    });

    let resp = client
        .post(format!("{}/chat/completions", api_base))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        anyhow::bail!("LLM request failed: HTTP {status} — {snippet}");
    }

    let json: serde_json::Value = resp.json().await?;
    // A 2xx with empty/null content is valid for some providers; do not bail.
    Ok(json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

/// Streaming chat-completions call: requests `"stream": true`, parses the SSE
/// body line-by-line, forwards each content delta as an `AgentEvent::Token`,
/// and returns the full accumulated text. Providers that ignore `stream` and
/// return a plain JSON body degrade gracefully to a single Token event.
pub(crate) async fn call_llm_stream(
    client: &reqwest::Client,
    api_base: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
    tx: &tokio::sync::mpsc::UnboundedSender<AgentEvent>,
) -> Result<String> {
    use futures::StreamExt;

    if api_key.trim().is_empty() {
        anyhow::bail!(
            "No API key configured for model '{}'. Set model.api_key (or the routed provider's key) first.",
            model
        );
    }

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": 0.7,
        "max_tokens": 8192,
        "stream": true,
    });

    let resp = client
        .post(format!("{}/chat/completions", api_base))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        anyhow::bail!("LLM request failed: HTTP {status} — {snippet}");
    }

    let mut full = String::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk?);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            match parse_sse_line(&line) {
                Some(SseLine::Done) => return Ok(full),
                Some(SseLine::Json(v)) => {
                    if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                        if !t.is_empty() {
                            full.push_str(t);
                            let _ = tx.send(AgentEvent::Token { text: t.to_string() });
                        }
                    }
                }
                None => {}
            }
        }
    }

    // The stream ended without [DONE]. The remaining buffer may hold a final
    // SSE line that arrived without a trailing newline — parse it first.
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        if let Some(SseLine::Json(v)) = parse_sse_line(&line) {
            if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                if !t.is_empty() {
                    full.push_str(t);
                    let _ = tx.send(AgentEvent::Token { text: t.to_string() });
                }
            }
            return Ok(full);
        }
    }

    // Provider ignored `stream` (or sent one unterminated JSON body): parse the
    // remaining buffer as a plain chat-completions response.
    if full.is_empty() && !buf.is_empty() {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf) {
            if let Some(t) = v["choices"][0]["message"]["content"].as_str() {
                full.push_str(t);
                let _ = tx.send(AgentEvent::Token { text: t.to_string() });
            }
        }
    }
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::permissions::{PermissionMode, PermissionRules};

    #[test]
    fn permission_key_wraps_shell_commands() {
        let call = ToolCall { name: "bash".into(), arguments: serde_json::json!({"command": "git push"}) };
        assert_eq!(Agent::permission_key(&call), "Bash(git push)");
        let call = ToolCall { name: "shell".into(), arguments: serde_json::json!({"command": "ls"}) };
        assert_eq!(Agent::permission_key(&call), "Bash(ls)");
        let call = ToolCall { name: "read".into(), arguments: serde_json::json!({"path": "x"}) };
        assert_eq!(Agent::permission_key(&call), "read");
        // self_manage's run_command action embeds a shell command — it must be
        // gated like bash, not like a benign self_manage call.
        let call = ToolCall {
            name: "self_manage".into(),
            arguments: serde_json::json!({"action": "run_command", "command": "rm -rf /"}),
        };
        assert_eq!(Agent::permission_key(&call), "Bash(rm -rf /)");
        let call = ToolCall { name: "self_manage".into(), arguments: serde_json::json!({"action": "list_skills"}) };
        assert_eq!(Agent::permission_key(&call), "self_manage");
    }

    #[test]
    fn deny_rule_on_raw_tool_name_blocks_shell_tools() {
        // A user writing deny: ["shell"] expects the shell tool blocked even
        // though its permission key is Bash(<command>).
        let rules = PermissionRules { allow: vec![], ask: vec![], deny: vec!["shell".into()] };
        let call = ToolCall { name: "shell".into(), arguments: serde_json::json!({"command": "ls"}) };
        let agent = Agent::new("m");
        assert!(agent.check_permission(&rules, &call).is_err());
    }

    #[test]
    fn deny_rule_beats_bypass_mode() {
        let rules = PermissionRules { allow: vec![], ask: vec![], deny: vec!["Bash(rm *)".into()] };
        let call = ToolCall { name: "bash".into(), arguments: serde_json::json!({"command": "rm -rf /"}) };
        let agent = Agent::new("m"); // default mode: BypassPermissions
        assert!(agent.check_permission(&rules, &call).is_err());
        let ok = ToolCall { name: "bash".into(), arguments: serde_json::json!({"command": "ls"}) };
        assert!(agent.check_permission(&rules, &ok).is_ok());
    }

    #[test]
    fn ask_resolves_to_refusal_headless() {
        let rules = PermissionRules::default();
        let agent = Agent::new("m").with_permission_mode(PermissionMode::Default);
        let write = ToolCall { name: "write".into(), arguments: serde_json::json!({}) };
        let err = agent.check_permission(&rules, &write).unwrap_err();
        assert!(err.contains("approval"), "headless Ask must explain itself: {}", err);
        let read = ToolCall { name: "read".into(), arguments: serde_json::json!({}) };
        assert!(agent.check_permission(&rules, &read).is_ok());
    }

    #[test]
    fn filtered_tools_keeps_only_requested() {
        let names: Vec<String> = Agent::filtered_tools(&["read".into(), "grep".into(), "nonexistent".into()])
            .iter().map(|t| t.name.clone()).collect();
        assert_eq!(names, vec!["grep".to_string(), "read".to_string()]);
    }

    #[tokio::test]
    async fn task_tool_refuses_nested_spawn() {
        let mut agent = Agent::new("m");
        agent.depth = 1;
        let out = agent
            .handle_task_tool(&serde_json::json!({"subagent_type": "Explore", "prompt": "x"}))
            .await
            .unwrap();
        assert!(out.contains("depth"), "must refuse with a depth explanation: {}", out);
    }

    #[test]
    fn agent_event_json_shape_is_stable() {
        // The frontends parse these exact shapes — this is the frozen contract.
        let ev = AgentEvent::Token { text: "hi".into() };
        assert_eq!(serde_json::to_string(&ev).unwrap(), r#"{"type":"token","text":"hi"}"#);
        let ev = AgentEvent::ToolStart { name: "bash".into(), summary: "ls".into() };
        assert_eq!(serde_json::to_string(&ev).unwrap(), r#"{"type":"tool_start","name":"bash","summary":"ls"}"#);
        let ev = AgentEvent::ToolEnd { name: "bash".into(), ok: true, preview: "x".into() };
        assert_eq!(serde_json::to_string(&ev).unwrap(), r#"{"type":"tool_end","name":"bash","ok":true,"preview":"x"}"#);
        let ev = AgentEvent::Done { text: "t".into() };
        assert_eq!(serde_json::to_string(&ev).unwrap(), r#"{"type":"done","text":"t"}"#);
        let ev = AgentEvent::Error { message: "m".into() };
        assert_eq!(serde_json::to_string(&ev).unwrap(), r#"{"type":"error","message":"m"}"#);
    }

    #[test]
    fn parse_sse_line_handles_data_done_and_noise() {
        assert!(matches!(parse_sse_line("data: [DONE]"), Some(SseLine::Done)));
        match parse_sse_line(r#"data: {"choices":[{"delta":{"content":"hey"}}]}"#) {
            Some(SseLine::Json(v)) => {
                assert_eq!(v["choices"][0]["delta"]["content"].as_str(), Some("hey"));
            }
            other => panic!("expected Json, got {:?}", other),
        }
        assert!(parse_sse_line(": keepalive").is_none());
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line("event: ping").is_none());
        assert!(parse_sse_line("data: not-json").is_none());
    }

    #[test]
    fn tool_summary_describes_common_tools() {
        let call = ToolCall { name: "bash".into(), arguments: serde_json::json!({"command": "cargo test"}) };
        assert_eq!(Agent::tool_summary(&call), "cargo test");
        let call = ToolCall { name: "read".into(), arguments: serde_json::json!({"path": "src/main.rs"}) };
        assert_eq!(Agent::tool_summary(&call), "src/main.rs");
        let call = ToolCall { name: "web_search".into(), arguments: serde_json::json!({"query": "rust sse"}) };
        assert_eq!(Agent::tool_summary(&call), "rust sse");
        let call = ToolCall { name: "goal_list".into(), arguments: serde_json::json!({}) };
        assert_eq!(Agent::tool_summary(&call), "{}");
    }

    #[tokio::test]
    async fn watch_tool_add_list_remove_via_execute_tool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = Agent::new("m").with_workspace(dir.path().to_str().unwrap().to_string());
        let mut ctx = crate::core::persona::FullContext::new();
        let mut session = crate::core::session::Session::new("test", "test");
        let mem = crate::core::memory::MemoryWorkspace::from_data_dir(dir.path().to_str().unwrap());

        let add = ToolCall {
            name: "watch".into(),
            arguments: serde_json::json!({"action": "add", "url": "https://example.com", "note": "demo", "interval_minutes": 15}),
        };
        let out = agent.execute_tool(&add, &mut ctx, &mut session, &mem).await.unwrap();
        assert!(out.contains("Watching https://example.com"), "{}", out);

        let list = ToolCall { name: "watch".into(), arguments: serde_json::json!({"action": "list"}) };
        let out = agent.execute_tool(&list, &mut ctx, &mut session, &mem).await.unwrap();
        assert!(out.contains("https://example.com") && out.contains("every 15m"), "{}", out);

        let rm = ToolCall { name: "watch".into(), arguments: serde_json::json!({"action": "remove", "url": "https://example.com"}) };
        let out = agent.execute_tool(&rm, &mut ctx, &mut session, &mem).await.unwrap();
        assert!(out.contains("Stopped watching"), "{}", out);

        // Rejects non-http URLs.
        let bad = ToolCall { name: "watch".into(), arguments: serde_json::json!({"action": "add", "url": "file:///etc/passwd"}) };
        let out = agent.execute_tool(&bad, &mut ctx, &mut session, &mem).await.unwrap();
        assert!(out.contains("http(s)"), "{}", out);
    }

    #[test]
    fn derive_fact_key_slugs_first_words() {
        assert_eq!(derive_fact_key("Prefers concise answers"), "prefers-concise-answers");
        assert_eq!(derive_fact_key("Works in Rust & Tauri, mostly!"), "works-in-rust-tauri-mostly");
        assert_eq!(derive_fact_key("!!!"), "fact");
        assert!(derive_fact_key("one two three four five six seven").split('-').count() <= 5);
    }

    #[tokio::test]
    async fn remember_tool_add_list_forget_via_execute_tool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = Agent::new("m").with_workspace(dir.path().to_str().unwrap().to_string());
        let mut ctx = crate::core::persona::FullContext::new();
        let mut session = crate::core::session::Session::new("test", "test");
        let mem = crate::core::memory::MemoryWorkspace::from_data_dir(dir.path().to_str().unwrap());

        let add = ToolCall {
            name: "remember".into(),
            arguments: serde_json::json!({"action": "add", "value": "Prefers concise answers", "importance": 0.8}),
        };
        let out = agent.execute_tool(&add, &mut ctx, &mut session, &mem).await.unwrap();
        assert!(out.contains("Remembered"), "{}", out);

        let list = ToolCall { name: "remember".into(), arguments: serde_json::json!({"action": "list"}) };
        let out = agent.execute_tool(&list, &mut ctx, &mut session, &mem).await.unwrap();
        assert!(out.contains("Prefers concise answers"), "{}", out);

        let forget = ToolCall {
            name: "remember".into(),
            arguments: serde_json::json!({"action": "forget", "key": "prefers-concise-answers"}),
        };
        let out = agent.execute_tool(&forget, &mut ctx, &mut session, &mem).await.unwrap();
        assert!(out.contains("Forgot"), "{}", out);

        // Missing value on add is handled, not panicked.
        let bad = ToolCall { name: "remember".into(), arguments: serde_json::json!({"action": "add"}) };
        let out = agent.execute_tool(&bad, &mut ctx, &mut session, &mem).await.unwrap();
        assert!(out.contains("provide a 'value'"), "{}", out);
    }

    fn hook_result(block: bool, modified: Option<serde_json::Value>, stderr: &str) -> crate::core::hooks::HookResult {
        crate::core::hooks::HookResult {
            hook_event: "pre_tool_use".into(),
            command: "x".into(),
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code: 0,
            duration_ms: 0,
            block,
            block_reason: None,
            modified_input: modified,
        }
    }

    #[test]
    fn decide_pre_tool_block_wins_over_modify() {
        let results = vec![
            hook_result(false, Some(serde_json::json!({"command": "ls"})), ""),
            hook_result(true, None, "not allowed"),
        ];
        let d = decide_pre_tool(&results);
        assert!(d.blocked);
        assert_eq!(d.reason, "not allowed");
        assert!(d.modified_input.is_none(), "a block clears any pending modify");
    }

    #[test]
    fn decide_pre_tool_applies_last_modify_when_not_blocked() {
        let results = vec![
            hook_result(false, Some(serde_json::json!({"command": "a"})), ""),
            hook_result(false, Some(serde_json::json!({"command": "b"})), ""),
        ];
        let d = decide_pre_tool(&results);
        assert!(!d.blocked);
        assert_eq!(d.modified_input, Some(serde_json::json!({"command": "b"})));
    }

    #[test]
    fn decide_pre_tool_empty_is_noop() {
        let d = decide_pre_tool(&[]);
        assert!(!d.blocked && d.modified_input.is_none());
    }

    #[test]
    fn load_hooks_is_gated_on_operator() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().to_str().unwrap();
        // Non-operator (default, e.g. a gateway agent): no hook engine at all.
        assert!(Agent::load_hooks_for(false, ws).is_none(), "remote agents must not load hooks");
        // Operator: an engine is loaded (empty here — no hooks.json).
        assert!(Agent::load_hooks_for(true, ws).is_some());
        // The builder sets the flag the loader gates on.
        assert!(Agent::new("m").operator().operator);
        assert!(!Agent::new("m").operator);
    }

    #[tokio::test]
    async fn hook_engine_fires_and_can_block() {
        use crate::core::hooks::{HookContext, HookEngine, HookEvent};
        // Skip where bash isn't available (the hook runner shells out to bash).
        if std::process::Command::new("bash").arg("-c").arg("true").status().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".claude/hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(
            hooks_dir.join("hooks.json"),
            r#"[{"event":"pre_tool_use","command":"echo '{\"block\":true}'","description":"deny","enabled":true}]"#,
        )
        .unwrap();

        let engine = HookEngine::load_from_workspace(dir.path().to_str().unwrap()).unwrap();
        let ctx = HookContext {
            session_id: "s".into(), workspace_dir: dir.path().to_string_lossy().into(),
            event: "pre_tool_use".into(), tool_name: Some("bash".into()),
            tool_input: None, tool_output: None, user_message: None,
            assistant_message: None, timestamp: "t".into(),
        };
        let results = engine.fire(&HookEvent::PreToolUse, &ctx).await;
        assert_eq!(results.len(), 1);
        assert!(decide_pre_tool(&results).blocked, "the hook returned block:true");
    }

    #[tokio::test]
    async fn standing_order_actions_inject_and_save_note() {
        use crate::core::standing_orders::{OrderAction, OrderTrigger, StandingOrder};
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().to_str().unwrap().to_string();
        let agent = Agent::new("m").with_workspace(ws.clone());
        let mem = crate::core::memory::MemoryWorkspace::from_data_dir(&ws);
        let mut ctx_out = Vec::new();

        let inject = StandingOrder {
            id: "i".into(), name: "i".into(), enabled: true,
            trigger: OrderTrigger::Keyword { phrases: vec!["x".into()] },
            action: OrderAction::InjectContext { text: "the user likes brevity".into() },
            description: String::new(),
        };
        agent.apply_order_action(&inject, "msg", 1, &mem, &mut ctx_out).await;
        assert_eq!(ctx_out, vec!["the user likes brevity".to_string()]);

        let note = StandingOrder {
            id: "n".into(), name: "n".into(), enabled: true,
            trigger: OrderTrigger::Keyword { phrases: vec!["x".into()] },
            action: OrderAction::SaveNote { template: "pref: {{message}}".into() },
            description: String::new(),
        };
        agent.apply_order_action(&note, "I like tea", 2, &mem, &mut ctx_out).await;
        assert!(mem.read_today().contains("pref: I like tea"), "SaveNote wrote to daily memory");

        // RunCommand is skipped for a non-operator agent (no panic, no run).
        let cmd = StandingOrder {
            id: "c".into(), name: "c".into(), enabled: true,
            trigger: OrderTrigger::Keyword { phrases: vec!["x".into()] },
            action: OrderAction::RunCommand { command: "echo should-not-run".into() },
            description: String::new(),
        };
        agent.apply_order_action(&cmd, "msg", 1, &mem, &mut ctx_out).await;
        assert_eq!(ctx_out.len(), 1, "RunCommand on a remote agent adds no context and is skipped");
    }

    #[test]
    fn truncate_output_respects_char_boundaries() {
        assert_eq!(truncate_output("short", 100), "short");
        let long = "é".repeat(100); // 2 bytes per char
        let cut = truncate_output(&long, 51); // 51 is mid-char
        assert!(cut.starts_with(&"é".repeat(25)));
        assert!(cut.contains("truncated"));
    }
}
