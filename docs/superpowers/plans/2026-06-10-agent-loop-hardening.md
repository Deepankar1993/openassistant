# Agent Loop Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the wildcard-permission bug, give the agent a multi-step tool loop, enforce permissions in tool dispatch (origin-aware), make `web_search` and `task` (sub-agent) tools real.

**Architecture:** All work centers on `src/core/agent.rs` (loop + dispatch refactor) and `src/core/permissions.rs` (fix + explicit-rule API), plus an additive `[permissions]` config section and a one-line builder call in each gateway channel. The text-based `[TOOL:name:{json}]` protocol is kept; the loop feeds each tool result back to the LLM until it answers without a tool call (max 6 rounds).

**Tech Stack:** Rust, tokio, serde/serde_yaml, reqwest, anyhow. Tests via `cargo test`.

Spec: `docs/superpowers/specs/2026-06-10-agent-loop-hardening-design.md`

---

### Task 1: Fix wildcard matching + add `check_explicit` (permissions.rs)

**Files:**
- Modify: `src/core/permissions.rs:142-158` (`matches`), `:123-139` (`check`)
- Tests: same file, `#[cfg(test)] mod tests`

- [ ] **Step 1: Confirm the two pre-existing tests fail**

Run: `cargo test -p open-assistant permissions -- --nocapture`
Expected: `test_wildcard_matching` and `test_permission_rules_priority` FAIL (assertion on `Bash(git push)` vs `Bash(git *)`).

- [ ] **Step 2: Add new failing tests** (append inside `mod tests`):

```rust
    #[test]
    fn test_wildcard_matching_non_bash() {
        assert!(PermissionRules::matches("mcp__github_search", "mcp__*"));
        assert!(!PermissionRules::matches("web_search", "mcp__*"));
    }

    #[test]
    fn test_check_explicit_distinguishes_no_rule_from_allow() {
        let rules = PermissionRules {
            allow: vec!["Read".into()],
            ask: vec![],
            deny: vec!["Bash(rm *)".into()],
        };
        assert!(matches!(rules.check_explicit("Read"), Some(PermissionAction::Allow)));
        assert!(matches!(rules.check_explicit("Bash(rm -rf /tmp)"), Some(PermissionAction::Deny(_))));
        assert!(rules.check_explicit("write").is_none(), "no rule => None, not Allow");
    }
```

- [ ] **Step 3: Implement.** `matches` must strip the trailing `)` whenever `Bash(` was stripped; `check` is reimplemented on top of a new `check_explicit`:

```rust
    /// Like `check`, but distinguishes "no rule matched" (None) from an
    /// explicit Allow — callers can fall through to mode-based decisions.
    pub fn check_explicit(&self, tool: &str) -> Option<PermissionAction> {
        if self.deny.iter().any(|d| Self::matches(tool, d)) {
            return Some(PermissionAction::Deny(format!("Tool '{}' is denied by settings", tool)));
        }
        if self.allow.iter().any(|a| Self::matches(tool, a)) {
            return Some(PermissionAction::Allow);
        }
        if self.ask.iter().any(|a| Self::matches(tool, a)) {
            return Some(PermissionAction::Ask(format!("Settings require approval for '{}'", tool)));
        }
        None
    }

    pub fn check(&self, tool: &str) -> PermissionAction {
        self.check_explicit(tool).unwrap_or(PermissionAction::Allow)
    }

    /// Match tool name against a rule (supports wildcards like "Bash(git *)")
    fn matches(tool: &str, rule: &str) -> bool {
        if rule == tool {
            return true;
        }
        // Bash rules compare command text: "Bash(git push)" ⇄ "Bash(git *)"
        let tool_base = tool.strip_prefix("Bash(").and_then(|s| s.strip_suffix(')')).unwrap_or(tool);
        let rule_base = rule.strip_prefix("Bash(").and_then(|s| s.strip_suffix(')')).unwrap_or(rule);
        if tool_base == rule_base {
            return true;
        }
        if rule_base.contains('*') {
            let prefix = rule_base.trim_end_matches('*');
            return tool_base.starts_with(prefix);
        }
        false
    }
```

(`matches` stays private; tests live in the same module so they can call it.)

- [ ] **Step 4: Run** `cargo test -p open-assistant permissions` — Expected: all permission tests PASS (10 total).

- [ ] **Step 5: Commit** `fix(permissions): strip trailing ')' in Bash(...) wildcard rules; add check_explicit`

### Task 2: `[permissions]` config section

**Files:**
- Modify: `src/config/mod.rs` (struct + `set()` + tests)

- [ ] **Step 1: Failing test** (append in `mod tests`):

```rust
    #[test]
    fn permissions_config_defaults_and_round_trip() {
        let cfg = Config::default();
        assert_eq!(cfg.permissions.gateway_mode, "acceptEdits");
        assert!(cfg.permissions.deny.is_empty());

        let mut cfg2 = Config::default();
        cfg2.permissions.deny = vec!["Bash(rm *)".into()];
        cfg2.permissions.gateway_mode = "default".into();
        let yaml = serde_yaml::to_string(&cfg2).expect("serialize");
        let back: Config = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(back.permissions.deny, vec!["Bash(rm *)".to_string()]);
        assert_eq!(back.permissions.gateway_mode, "default");

        // Legacy YAML without a `permissions:` key must still load (serde default).
        let legacy = "model:\n  provider: openrouter\n  model: m\n  api_key: ''\n  api_base: https://x\n";
        let cfg3: Config = serde_yaml::from_str(legacy).expect("legacy loads");
        assert_eq!(cfg3.permissions.gateway_mode, "acceptEdits");
    }
```

- [ ] **Step 2: Run to verify it fails** (compile error: no field `permissions`).

- [ ] **Step 3: Implement.** Add to `Config` (after the `claude` field):

```rust
    /// Built-in tool permission posture. Rules apply at every mode (deny beats
    /// bypass); `gateway_mode` caps remote channels (Discord/Telegram/Slack/
    /// WebChat) the same way the claude bridge caps remote origins.
    #[serde(default)]
    pub permissions: PermissionsConfig,
```

New struct (near `ToolsConfig`):

```rust
/// Permission posture for the built-in agent's tools. Local front-ends keep
/// full autonomy (BypassPermissions) to preserve pre-existing behavior; the
/// gateway constructs agents with `gateway_mode` instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionsConfig {
    /// Permission mode for remote gateway channels: default | acceptEdits |
    /// auto | bypassPermissions.
    pub gateway_mode: String,
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            gateway_mode: "acceptEdits".to_string(),
            allow: vec![],
            ask: vec![],
            deny: vec![],
        }
    }
}
```

Add to `set()` (before the `_ =>` arm); list values are comma-separated:

```rust
        "permissions.gateway_mode" => config.permissions.gateway_mode = value.to_string(),
        "permissions.allow" => config.permissions.allow = split_list(value),
        "permissions.ask" => config.permissions.ask = split_list(value),
        "permissions.deny" => config.permissions.deny = split_list(value),
```

with a helper next to `default_data_dir()`:

```rust
fn split_list(value: &str) -> Vec<String> {
    value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}
```

(Also replace the inline split in `gateway.discord_allowed_users` with `split_list(value)` — same behavior, DRY.)

- [ ] **Step 4: Run** `cargo test -p open-assistant config` — Expected: PASS.

- [ ] **Step 5: Commit** `feat(config): additive [permissions] section (gateway_mode + allow/ask/deny rules)`

### Task 3: Refactor dispatch into `execute_tool` + permission/truncation helpers

**Files:**
- Modify: `src/core/agent.rs` (`Agent` struct, `handle_tool_calls` → `execute_tool`, new helpers)
- Tests: new `#[cfg(test)] mod tests` at the bottom of `agent.rs`

- [ ] **Step 1: Failing tests** (new module at the bottom of `agent.rs`, above nothing — file currently has no test module):

```rust
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
    fn truncate_output_respects_char_boundaries() {
        assert_eq!(truncate_output("short", 100), "short");
        let long = "é".repeat(100); // 2 bytes per char
        let cut = truncate_output(&long, 51); // 51 is mid-char
        assert!(cut.starts_with(&"é".repeat(25)));
        assert!(cut.contains("truncated"));
    }
}
```

- [ ] **Step 2: Run to verify failure** (compile errors: `with_permission_mode`, `permission_key`, `check_permission`, `truncate_output` undefined).

- [ ] **Step 3: Implement.**

3a. `Agent` struct gains two fields (and `new()` initializes them):

```rust
    /// Permission posture for tool dispatch. Local front-ends keep the
    /// pre-existing full-autonomy behavior (BypassPermissions); gateway
    /// channels cap this via `with_permission_mode` (origin-aware, like the
    /// claude bridge). Config deny/ask rules apply at EVERY mode.
    pub permission_mode: super::permissions::PermissionMode,
    /// Sub-agent nesting depth: 0 = top-level. Sub-agents get depth+1 and
    /// refuse to spawn further sub-agents.
    pub depth: u8,
```

In `new()`: `permission_mode: super::permissions::PermissionMode::BypassPermissions, depth: 0,`. Builder:

```rust
    pub fn with_permission_mode(mut self, mode: super::permissions::PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }
```

3b. Helpers (associated/instance fns on `Agent`, plus one free fn):

```rust
    /// Key used for rule matching: bash/shell check as `Bash(<command>)` so
    /// command wildcards like `Bash(git *)` work; other tools check by name.
    fn permission_key(tool_call: &ToolCall) -> String {
        match tool_call.name.as_str() {
            "bash" | "shell" => {
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
        let key = Self::permission_key(tool_call);
        let action = match rules.check_explicit(&key) {
            Some(a) => a,
            None => self.permission_mode.check_action(&tool_call.name, &tool_call.arguments),
        };
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
```

Free function (above `push_str`):

```rust
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
```

3c. Rename `handle_tool_calls` → `execute_tool(&self, tool_call: &ToolCall, ctx, session, mem) -> Result<String>`: delete the `parse_tool_call` wrapper and the `match`'s `response` concatenation. Every arm returns ONLY the tool output, e.g.:

```rust
            "bash" => {
                let result = crate::tools::bash::execute(&tool_call.arguments).await?;
                if result.success {
                    Ok(format!("Bash output:\n{}", result.output))
                } else {
                    Ok(format!("Bash failed:\n{}\n{}", result.output, result.error.unwrap_or_default()))
                }
            }
```

Same mechanical change for every arm (`read` → `"File content:\n{}"`, `write` → `"Write result:\n{}"`, `edit` → `"Edit result: {}"` / `"Edit error: ..."`, `glob` → `"Glob results ({} files):\n{}"`, `grep` → `"Grep results (...):\n..."`, `shell`/`file`/`browser`/`vision` analogous, handlers like `handle_todo_write` already return bare output). The unknown-tool arm becomes:

```rust
                _ => {
                    tracing::warn!("Unknown tool: {}", tool_call.name);
                    Ok(format!("Unknown tool '{}'. Available tools are listed in the system prompt.", tool_call.name))
                }
```

`process()` temporarily keeps single-shot behavior by inlining the old shape (replaced in Task 4):

```rust
        let final_response = if self.tools_enabled {
            if let Some(tool_call) = self.parse_tool_call(&response) {
                let out = self.execute_tool(&tool_call, ctx, session, &mem).await?;
                format!("{}\n\n{}", response, out)
            } else {
                response
            }
        } else {
            response
        };
```

- [ ] **Step 4: Run** `cargo test -p open-assistant` — Expected: all PASS (incl. 4 new agent tests).

- [ ] **Step 5: Commit** `refactor(agent): execute_tool returns bare output; permission gate + output truncation helpers`

### Task 4: Multi-step tool loop in `process()`

**Files:**
- Modify: `src/core/agent.rs` (`process()`, constants)

- [ ] **Step 1: Implement** (no unit test possible without an HTTP mock layer — verified by the full test suite compiling and by live use; the loop's helpers were tested in Task 3). Constants near the top of the impl/file:

```rust
/// Max LLM⇄tool rounds per user turn.
const MAX_TOOL_ITERATIONS: usize = 6;
/// Max bytes of a single tool output fed back to the model.
const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;
```

Replace the Task-3 single-shot block in `process()` (everything from `let response = self.call_llm(...)`) with:

```rust
        // Call the LLM, then loop: execute the requested tool, feed the result
        // back, and let the model continue until it answers without a tool
        // call (or the iteration cap is hit).
        let mut messages = messages;
        let mut response = self.call_llm(&messages).await?;

        let final_response = if self.tools_enabled {
            let config = crate::config::load().await?;
            let rules = super::permissions::PermissionRules {
                allow: config.permissions.allow.clone(),
                ask: config.permissions.ask.clone(),
                deny: config.permissions.deny.clone(),
            };
            let mut iterations = 0;
            while let Some(tool_call) = self.parse_tool_call(&response) {
                if iterations >= MAX_TOOL_ITERATIONS {
                    response.push_str("\n\n[Stopped: tool iteration limit reached for this turn.]");
                    break;
                }
                iterations += 1;
                debug!("Tool round {}: {}", iterations, tool_call.name);

                let output = match self.check_permission(&rules, &tool_call) {
                    Ok(()) => match self.execute_tool(&tool_call, ctx, session, &mem).await {
                        Ok(out) => out,
                        // Execution errors go back to the model as text so it
                        // can recover; only transport/config errors from
                        // call_llm abort the turn.
                        Err(e) => format!("Tool '{}' failed: {}", tool_call.name, e),
                    },
                    Err(denial) => denial,
                };
                let output = truncate_output(&output, MAX_TOOL_OUTPUT_BYTES);

                // Record the trajectory in both the working message list and
                // the session, so later turns keep the tool context.
                let result_msg = format!("[TOOL RESULT: {}]\n{}", tool_call.name, output);
                session.add_message(Message::assistant(&response));
                session.add_message(Message::user(&result_msg));
                messages.push(serde_json::json!({"role": "assistant", "content": response}));
                messages.push(serde_json::json!({"role": "user", "content": result_msg}));

                response = self.call_llm(&messages).await?;
            }
            response
        } else {
            response
        };
```

Also extend the tool-instructions block in `build_system_prompt` (inside the `if self.tools_enabled` branch, after the tool list) so the model knows about the loop:

```rust
            push_str(&mut prompt, "After you emit a tool call, STOP — the tool result will be sent back to you as a [TOOL RESULT: name] message, and you can then continue or emit the next tool call. One tool call per message.");
```

- [ ] **Step 2: Run** `cargo build -p open-assistant && cargo test -p open-assistant` — Expected: build + all tests PASS.

- [ ] **Step 3: Commit** `feat(agent): multi-step tool loop — feed results back to the model (max 6 rounds, permission-gated)`

### Task 5: Real `web_search` tool

**Files:**
- Modify: `src/core/agent.rs` (`web_search` arm in `execute_tool`)

- [ ] **Step 1: Implement** — replace the URL-formatting arm:

```rust
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
```

- [ ] **Step 2: Run** `cargo build -p open-assistant` — Expected: compiles. (Network behavior exercised live; errors degrade to text by design.)

- [ ] **Step 3: Commit** `feat(agent): wire web_search tool to the real multi-engine WebSearch implementation`

### Task 6: Real `task` sub-agent

**Files:**
- Modify: `src/core/agent.rs` (`handle_task_tool`, new `filtered_tools` helper)
- Tests: `mod tests` in `agent.rs`

- [ ] **Step 1: Failing tests:**

```rust
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
```

(`filtered_tools` sorts by name for deterministic assertion.)

- [ ] **Step 2: Run to verify failure** (compile error: `filtered_tools` undefined; depth refusal missing).

- [ ] **Step 3: Implement.** Helper:

```rust
    /// Restrict the default tool set to an allowlist (sub-agent tool scoping).
    fn filtered_tools(allowed: &[String]) -> Vec<ToolDefinition> {
        let mut tools: Vec<ToolDefinition> = Self::default_tools()
            .into_iter()
            .filter(|t| allowed.iter().any(|a| a == &t.name))
            .collect();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }
```

Replace the body of `handle_task_tool`:

```rust
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
```

- [ ] **Step 4: Run** `cargo test -p open-assistant` — Expected: PASS (the nested-refusal test never reaches an LLM call).

- [ ] **Step 5: Commit** `feat(agent): task tool spawns a real in-process sub-agent (scoped tools, depth limit 1)`

### Task 7: Gateway permission-mode wiring

**Files:**
- Modify: `src/gateway/discord.rs:362`, `src/gateway/telegram.rs:29`, `src/gateway/webchat.rs:93`

- [ ] **Step 1: Implement** — in each of the three constructions add one builder call (Slack runs inside the WebChat server and shares its agent):

```rust
    let agent = Agent::new(config.model.model.clone())
        .with_workspace(/* unchanged per file */)
        .with_tools_enabled(config.tools.enabled)
        .with_permission_mode(crate::core::permissions::PermissionMode::from_str(
            &config.permissions.gateway_mode,
        ));
```

(Use the existing import style of each file; `crate::core::permissions::PermissionMode` fully qualified is fine.)

- [ ] **Step 2: Run** `cargo build -p open-assistant && cargo test -p open-assistant` — Expected: PASS.

- [ ] **Step 3: Commit** `feat(gateway): cap remote channels with permissions.gateway_mode (origin-aware, default acceptEdits)`

### Task 8: Docs + full verification

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update CLAUDE.md:** (a) tests note — remove the "two pre-existing tests FAIL" caveat; (b) gateway line — `start_gateway()` wires up all four channels (WebChat, Discord, Telegram, Slack); (c) agent-loop section — tool calling now loops up to 6 rounds feeding results back, permission-gated via `[permissions]` config + origin-aware gateway mode; `task` spawns a real in-process sub-agent (depth 1); `web_search` is real; (d) stub list — drop `task` from the stub examples, keep `goal_deliberate` accuracy (it was already real — say so).

- [ ] **Step 2: Full run:** `cargo test --workspace` — Expected: 0 failures. `cargo clippy -p open-assistant 2>&1 | Select-String "^warning" | Measure-Object` — Expected: no NEW warnings vs. baseline (78).

- [ ] **Step 3: Commit** `docs: CLAUDE.md reflects multi-step tool loop, enforced permissions, wired gateway channels`

## Self-review

- Spec coverage: §1→Task 1, §2→Tasks 3+4, §3→Tasks 2+3+4+7, §4→Task 5, §5→Task 6, docs→Task 8. ✔
- No placeholders; every code step shows the code. ✔
- Type consistency: `check_explicit` (Task 1) is what `check_permission` (Task 3) calls; `with_permission_mode`/`depth` (Task 3) are what Task 6/7 use; `PermissionsConfig` field names (Task 2) match Task 4's `config.permissions.*` reads. ✔
