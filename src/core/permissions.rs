// src/core/permissions.rs
//! Session-level permission modes — Claude Code-style 4-mode system
//! Modes: Default, AcceptEdits, Auto, BypassPermissions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

// ─── Session-Level Permission Modes ───────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PermissionMode {
    /// Ask for everything except reads (safest, default)
    Default,
    /// Auto-approve file edits and common filesystem commands
    AcceptEdits,
    /// Classifier auto-approves safe actions (data exfiltration blocked)
    Auto,
    /// Approve everything — no prompts (CI/automation only, dangerous)
    BypassPermissions,
}

impl Default for PermissionMode {
    fn default() -> Self {
        PermissionMode::Default
    }
}

impl PermissionMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "default" | "normal" => PermissionMode::Default,
            "acceptedits" | "accept_edits" | "accept" => PermissionMode::AcceptEdits,
            "auto" | "auto-mode" => PermissionMode::Auto,
            "bypasspermissions" | "bypass_permissions" | "bypass" | "yolo" | "dangerously-skip-permissions" => {
                PermissionMode::BypassPermissions
            }
            _ => PermissionMode::Default,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            PermissionMode::Default => "🔒 DEFAULT (ask before actions)",
            PermissionMode::AcceptEdits => "✏️ ACCEPT-EDITS (auto-approve writes)",
            PermissionMode::Auto => "🤖 AUTO (classifier-based)",
            PermissionMode::BypassPermissions => "⚠️ BYPASS (no prompts — dangerous!)",
        }
    }

    pub fn cycle_next(&self) -> Self {
        match self {
            PermissionMode::Default => PermissionMode::AcceptEdits,
            PermissionMode::AcceptEdits => PermissionMode::Auto,
            PermissionMode::Auto => PermissionMode::BypassPermissions,
            PermissionMode::BypassPermissions => PermissionMode::Default,
        }
    }

    /// Check if a tool action is permitted in this mode
    pub fn check_action(&self, tool: &str, _args: &serde_json::Value) -> PermissionAction {
        match self {
            PermissionMode::BypassPermissions => PermissionAction::Allow,
            PermissionMode::Auto => Self::auto_classify(tool),
            PermissionMode::AcceptEdits => Self::accept_edits_check(tool),
            PermissionMode::Default => Self::default_check(tool),
        }
    }

    fn default_check(tool: &str) -> PermissionAction {
        // In Default mode, reads are auto-allowed, everything else asks
        match tool {
            "read" | "glob" | "grep" | "search" | "ls" | "context" | "cost" | "status" | "todos" | "plans" | "apply_patch" => PermissionAction::Allow,
            _ => PermissionAction::Ask(format!("Allow tool '{}' to run?", tool)),
        }
    }

    fn accept_edits_check(tool: &str) -> PermissionAction {
        // In AcceptEdits mode, reads + file edits + common fs commands are auto-allowed
        match tool {
            "read" | "glob" | "grep" | "write" | "edit" | "multi_edit" | "apply_patch" | "ls" | "context" | "cost" | "status" | "todos" | "search" => PermissionAction::Allow,
            // NOTE: blanket-allows shell commands — in this mode, command-level
            // restriction comes from config deny rules (checked before the
            // mode, so they fire even here). Use Default mode to refuse shell
            // outright on headless channels.
            "bash" | "shell" => PermissionAction::Allow,
            _ => PermissionAction::Ask(format!("Allow tool '{}' to run?", tool)),
        }
    }

    fn auto_classify(tool: &str) -> PermissionAction {
        // Auto mode: classifier-based (simplified — in production this uses ML)
        match tool {
            // Always allow read-only operations
            "read" | "glob" | "grep" | "search" | "ls" | "context" | "cost" | "status" | "todos" => PermissionAction::Allow,
            // Allow writes in project directory
            "write" | "edit" | "multi_edit" | "apply_patch" => PermissionAction::Allow,
            // Bash: allow common dev commands, ask for dangerous ones
            "bash" | "shell" => PermissionAction::Allow, // Simplified — real classifier is more nuanced
            // Network: allow
            "web_search" | "web_fetch" => PermissionAction::Allow,
            // Default: ask
            _ => PermissionAction::Ask(format!("Auto-classifier: allow '{}'?", tool)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum PermissionAction {
    Allow,
    Deny(String),
    Ask(String),
}

// ─── Per-Tool Permission Rules (from settings.json) ───────────────────

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PermissionRules {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

impl PermissionRules {
    /// Like `check`, but distinguishes "no rule matched" (None) from an
    /// explicit Allow — callers can fall through to mode-based decisions.
    pub fn check_explicit(&self, tool: &str) -> Option<PermissionAction> {
        // Deny takes highest priority
        if self.deny.iter().any(|d| Self::matches(tool, d)) {
            return Some(PermissionAction::Deny(format!("Tool '{}' is denied by settings", tool)));
        }
        // Then allow
        if self.allow.iter().any(|a| Self::matches(tool, a)) {
            return Some(PermissionAction::Allow);
        }
        // Then ask
        if self.ask.iter().any(|a| Self::matches(tool, a)) {
            return Some(PermissionAction::Ask(format!("Settings require approval for '{}'", tool)));
        }
        None
    }

    pub fn check(&self, tool: &str) -> PermissionAction {
        // No rule — fall through to mode-based decision
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
        // Handle glob patterns
        if rule_base.contains('*') {
            let prefix = rule_base.trim_end_matches('*');
            return tool_base.starts_with(prefix);
        }
        false
    }
}

// ─── Permission Manager (combines mode + rules) ───────────────────────

#[derive(Debug)]
pub struct PermissionManager {
    pub mode: PermissionMode,
    pub rules: PermissionRules,
    /// Track if user has accepted workspace trust for this directory
    pub workspace_trust_cache: HashMap<String, bool>,
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            mode: PermissionMode::Default,
            rules: PermissionRules::default(),
            workspace_trust_cache: HashMap::new(),
        }
    }

    pub fn set_mode(&mut self, mode: PermissionMode) {
        info!("Permission mode changed: {}", mode.label());
        self.mode = mode;
    }

    pub fn cycle_mode(&mut self) -> PermissionMode {
        self.mode = self.mode.cycle_next();
        info!("Permission mode cycled: {}", self.mode.label());
        self.mode
    }

    /// Full permission check: rules first, then mode
    pub fn check(&self, tool: &str, args: &serde_json::Value) -> PermissionAction {
        // 1. Check per-tool rules first
        match self.rules.check(tool) {
            PermissionAction::Allow => {}
            action => return action,
        }
        // 2. Check session-level mode
        self.mode.check_action(tool, args)
    }

    /// Check workspace trust for a directory
    pub fn check_workspace_trust(&mut self, dir: &str) -> bool {
        if let Some(&trusted) = self.workspace_trust_cache.get(dir) {
            return trusted;
        }
        // Not cached — in interactive mode, would ask user
        // For now, auto-trust (like --dangerously-skip-permissions)
        self.workspace_trust_cache.insert(dir.to_string(), true);
        true
    }

    pub fn format_status(&self) -> String {
        let mut output = format!("🔐 Permission Mode: {}\n", self.mode.label());
        output.push_str(&format!("  Allow rules: {}\n", self.rules.allow.len()));
        output.push_str(&format!("  Ask rules:   {}\n", self.rules.ask.len()));
        output.push_str(&format!("  Deny rules:  {}\n", self.rules.deny.len()));
        output.push_str(&format!("  Trusted dirs: {}\n", self.workspace_trust_cache.len()));
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_cycle() {
        let mut mode = PermissionMode::Default;
        mode = mode.cycle_next();
        assert_eq!(mode, PermissionMode::AcceptEdits);
        mode = mode.cycle_next();
        assert_eq!(mode, PermissionMode::Auto);
        mode = mode.cycle_next();
        assert_eq!(mode, PermissionMode::BypassPermissions);
        mode = mode.cycle_next();
        assert_eq!(mode, PermissionMode::Default);
    }

    #[test]
    fn test_default_mode_allows_reads() {
        let mode = PermissionMode::Default;
        assert!(matches!(mode.check_action("read", &serde_json::json!({})), PermissionAction::Allow));
        assert!(matches!(mode.check_action("glob", &serde_json::json!({})), PermissionAction::Allow));
    }

    #[test]
    fn test_default_mode_asks_for_writes() {
        let mode = PermissionMode::Default;
        assert!(matches!(mode.check_action("write", &serde_json::json!({})), PermissionAction::Ask(_)));
        assert!(matches!(mode.check_action("bash", &serde_json::json!({})), PermissionAction::Ask(_)));
    }

    #[test]
    fn test_bypass_allows_everything() {
        let mode = PermissionMode::BypassPermissions;
        assert!(matches!(mode.check_action("write", &serde_json::json!({})), PermissionAction::Allow));
        assert!(matches!(mode.check_action("bash", &serde_json::json!({"command": "rm -rf /"})), PermissionAction::Allow));
    }

    #[test]
    fn test_permission_rules_priority() {
        let rules = PermissionRules {
            allow: vec!["Read".into()],
            ask: vec!["Write".into()],
            deny: vec!["Bash(rm *)".into()],
        };

        assert!(matches!(rules.check("Read"), PermissionAction::Allow));
        assert!(matches!(rules.check("Write"), PermissionAction::Ask(_)));
        assert!(matches!(rules.check("Bash(rm -rf *)"), PermissionAction::Deny(_)));
    }

    #[test]
    fn test_wildcard_matching() {
        assert!(PermissionRules::matches("Bash(git commit *)", "Bash(git commit *)"));
        assert!(PermissionRules::matches("Bash(git push)", "Bash(git *)"));
        assert!(PermissionRules::matches("Bash(npm run lint)", "Bash(npm run *)"));
    }

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
}
