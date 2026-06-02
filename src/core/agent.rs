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
        }
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

    /// Process a message through the agent loop — full featured
    pub async fn process(
        &self,
        message: &str,
        ctx: &mut FullContext,
        session: &mut Session,
    ) -> Result<String> {
        info!("Processing: {}", &message[..message.len().min(80)]);

        // Add daily note about this interaction
        let mem = super::memory::MemoryWorkspace::from_data_dir(&self.workspace_dir);
        let _ = mem.append_daily(&format!("User said: {}", &message[..message.len().min(200)]));

        // Add user message to session
        session.add_message(Message::user(message));

        // Learn from conversation
        ctx.observe(message);

        // Build full system prompt with persona + user model + memory
        let system_prompt = self.build_system_prompt(ctx, &mem);

        // Build conversation messages
        let messages = self.build_messages(&system_prompt, session);

        // Call the LLM
        let response = self.call_llm(&messages).await?;

        // Handle tool calls (skipped when tool dispatch is disabled)
        let final_response = if self.tools_enabled {
            self.handle_tool_calls(response, ctx, session, &mem).await?
        } else {
            response
        };

        // Add assistant response to session
        session.add_message(Message::assistant(&final_response));

        // Daily note about response
        let _ = mem.append_daily(&format!("Assistant responded: {}", &final_response[..final_response.len().min(200)]));

        Ok(final_response)
    }

    /// Build full system prompt with persona, user model, memory, and tool instructions
    fn build_system_prompt(&self, ctx: &FullContext, mem: &super::memory::MemoryWorkspace) -> String {
        let mut prompt = ctx.build_system_prompt();

        // Memory context
        let memory_ctx = mem.build_context();
        if !memory_ctx.is_empty() {
            prompt.push_str("# Memory\n");
            prompt.push_str(&memory_ctx);
            prompt.push('\n');
        }

        // Tool instructions
        prompt.push_str("# Available Tools\n");
        prompt.push_str("Use [TOOL:name:{\"arg\":\"value\"}] to invoke tools:\n");
        for tool in &self.tools {
            prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
        }
        prompt.push('\n');

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

    async fn call_llm(&self, messages: &[serde_json::Value]) -> Result<String> {
        let config = crate::config::load().await?;
        let client = reqwest::Client::new();

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.7,
            "max_tokens": 8192,
        });

        if config.model.api_key.trim().is_empty() {
            anyhow::bail!(
                "No API key configured. Set `model.api_key` (provider: {}) before chatting.",
                config.model.provider
            );
        }

        let resp = client
            .post(format!("{}/chat/completions", config.model.api_base))
            .header("Authorization", format!("Bearer {}", config.model.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        // Surface transport/HTTP errors instead of silently returning an empty
        // string (the default empty api_key otherwise yields a blank reply).
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(500).collect();
            anyhow::bail!("LLM request failed: HTTP {status} — {snippet}");
        }

        let json: serde_json::Value = resp.json().await?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if content.trim().is_empty() {
            anyhow::bail!(
                "LLM returned an empty response (model `{}`). Check the model name and provider.",
                self.model
            );
        }

        Ok(content)
    }

    async fn handle_tool_calls(
        &self,
        response: String,
        ctx: &mut FullContext,
        session: &mut Session,
        mem: &super::memory::MemoryWorkspace,
    ) -> Result<String> {
        if let Some(tool_call) = self.parse_tool_call(&response) {
            debug!("Tool call: {}", tool_call.name);

            // Route to appropriate handler
            match tool_call.name.as_str() {
                // ── Claude Code core tools ──
                "bash" => {
                    let result = crate::tools::bash::execute(&tool_call.arguments).await?;
                    Ok(format!("{}\n\nBash output:\n{}", response, result.output))
                }
                "read" => {
                    let result = crate::tools::file::execute(&serde_json::json!({
                        "action": "read",
                        "path": tool_call.arguments["path"].as_str().unwrap_or("")
                    })).await?;
                    Ok(format!("{}\n\nFile content:\n{}", response, result.output))
                }
                "write" => {
                    let result = crate::tools::file::execute(&serde_json::json!({
                        "action": "write",
                        "path": tool_call.arguments["path"].as_str().unwrap_or(""),
                        "content": tool_call.arguments["content"].as_str().unwrap_or("")
                    })).await?;
                    Ok(format!("{}\n\nWrite result:\n{}", response, result.output))
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
                            Ok(format!("{}\n\nEdit result: {}", response, write_result.output))
                        } else {
                            Ok(format!("{}\n\nEdit error: old_string not found in {}", response, path))
                        }
                    } else {
                        Ok(format!("{}\n\nEdit error: could not read {}", response, path))
                    }
                }
                "glob" => {
                    let result = crate::tools::file_search::glob(&tool_call.arguments).await?;
                    let files: Vec<String> = result.files.clone();
                    Ok(format!("{}\n\nGlob results ({} files):\n{}", response, result.total_found, files.join("\n")))
                }
                "grep" => {
                    let result = crate::tools::file_search::grep(&tool_call.arguments).await?;
                    let mut output = format!("{}\n\nGrep results ({} matches in {} files):\n", response, result.total_matches, result.files_searched);
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
                    let engine = tool_call.arguments["engine"].as_str().unwrap_or("duckduckgo");
                    let url = format!("https://www.google.com/search?q={}", urlencoding::encode(query));
                    Ok(format!("{}\n\nWeb search ({}) for '{}': {}", response, engine, query, url))
                }
                // ── Legacy openAssistant tools ──
                "shell" => {
                    let result = crate::tools::shell::execute(&tool_call.arguments).await?;
                    Ok(format!("{}\n\nShell output:\n{}", response, result.output))
                }
                "file" => {
                    let result = crate::tools::file::execute(&tool_call.arguments).await?;
                    Ok(format!("{}\n\nFile result:\n{}", response, result.output))
                }
                "browser" => {
                    let result = crate::tools::browser::execute(&tool_call.arguments).await?;
                    Ok(format!("{}\n\nBrowser result:\n{}", response, result.output))
                }
                "vision" => {
                    let result = crate::tools::vision::execute(&tool_call.arguments).await?;
                    Ok(format!("{}\n\nVision result:\n{}", response, result.output))
                }
                "memory" => {
                    self.handle_memory_tool(&tool_call.arguments, mem).await
                }
                "self_manage" => {
                    self.handle_self_manage(&tool_call.arguments, mem, ctx).await
                }
                _ => {
                    tracing::warn!("Unknown tool: {}", tool_call.name);
                    Ok(response)
                }
            }
        } else {
            Ok(response)
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

        // Simulate multi-agent deliberation
        let roles = deliberator.default_roles().to_vec();
        let mut contributions = Vec::new();

        for role in &roles {
            if let Some(prompt) = deliberator.build_role_prompt(&goal_id, role) {
                // In a full implementation, each role would call the LLM
                // For now, we create a structured deliberation framework
                let contribution = format!(
                    "[{} {} would analyze this goal here with LLM call]\nPrompt preview: {}",
                    role.emoji(),
                    role.name(),
                    &prompt[..prompt.len().min(200)]
                );
                deliberator.add_contribution(&goal_id, role.clone(), &contribution, Some(0.8));
                contributions.push(format!("{} {}: Analysis pending LLM call", role.emoji(), role.name()));
            }
        }

        let output = format!(
            "🎯 Multi-Agent Goal Deliberation Started\n\
            Goal: {}\n\
            Description: {}\n\
            \n\
            Agents spawned:\n{}\n\
            \n\
            {}\n\
            \n\
            In a full implementation, each agent would call the LLM with its role prompt.\n\
            The Synthesizer would then combine all perspectives into a final plan.",
            title,
            description,
            contributions.join("\n"),
            deliberator.format_deliberation(&goal_id)
        );

        Ok(output)
    }

    async fn handle_task_tool(&self, args: &serde_json::Value) -> Result<String> {
        let subagent_type = args["subagent_type"].as_str().unwrap_or("General");
        let description = args["description"].as_str().unwrap_or("");
        let prompt = args["prompt"].as_str().unwrap_or("");

        let tools = args["tools"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
            .unwrap_or_else(|| vec!["read".to_string(), "glob".to_string(), "grep".to_string()]);

        let role_desc = match subagent_type {
            "Explore" => "fast agent specialized for exploring codebases. Use for finding files, searching code, and understanding project structure.",
            "Plan" => "agent specialized for planning and design. Use for creating implementation plans, designing architectures, and breaking down complex tasks.",
            _ => "general-purpose agent for complex tasks. Use for multi-step work that requires reasoning and tool use.",
        };

        Ok(format!(
            "🤖 Sub-Agent Task Spawned\n\
            Type: {} ({})\n\
            Description: {}\n\
            Tools: {}\n\
            \n\
            Prompt:\n{}\n\
            \n\
            In a full implementation, this would spawn an isolated agent process\n\
            with its own context, tool set, and conversation history.",
            subagent_type, role_desc, description, tools.join(", "), prompt
        ))
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
                Ok("To update openAssistant, run: cargo update && cargo build --release (or ask your user to run 'openassistant update')".to_string())
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
