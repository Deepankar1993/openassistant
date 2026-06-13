// src/core/standing_orders.rs
//! Standing orders + task flow (OpenClaw-style)
//! Recurring instructions and event-triggered workflows

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, debug};

/// A standing order — a persistent instruction that triggers on conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandingOrder {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger: OrderTrigger,
    pub action: OrderAction,
    pub description: String,
}

/// When does the standing order fire?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderTrigger {
    /// Fire when a keyword/phrase is detected in user message
    Keyword { phrases: Vec<String> },
    /// Fire after every session
    SessionEnd,
    /// Fire when a tool is used
    ToolUsed { tool_name: String },
    /// Fire when a specific event occurs
    Event { event: String },
    /// Fire on a schedule (uses cron expression)
    Schedule { cron: String },
    /// Fire when the agent starts
    OnBoot,
    /// Fire periodically (every N messages)
    EveryNMessages { count: usize },
}

/// What action to take
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderAction {
    /// Add context to the system prompt
    InjectContext { text: String },
    /// Save a note
    SaveNote { template: String },
    /// Run a tool
    RunTool { tool_name: String, arguments: serde_json::Value },
    /// Send a message
    SendMessage { text: String },
    /// Execute a skill
    RunSkill { skill_name: String },
    /// Run a shell command
    RunCommand { command: String },
    /// Webhook
    Webhook { url: String, body: String },
}

/// Standing orders engine
#[derive(Debug, Default)]
pub struct StandingOrdersEngine {
    orders: Vec<StandingOrder>,
}

impl StandingOrdersEngine {
    pub fn new() -> Self {
        let mut engine = Self::default();
        engine.load_defaults();
        engine
    }

    fn store_path(data_dir: &str) -> PathBuf {
        PathBuf::from(data_dir).join("standing_orders.json")
    }

    /// Load from `<data_dir>/standing_orders.json`. If absent, seed the
    /// defaults and persist them so the file becomes user-editable; a corrupt
    /// file falls back to defaults (logged).
    pub fn load(data_dir: &str) -> Self {
        let path = Self::store_path(data_dir);
        match std::fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str::<Vec<StandingOrder>>(&s) {
                Ok(orders) => Self { orders },
                Err(e) => {
                    info!("standing_orders.json unreadable ({e}); using defaults");
                    Self::new()
                }
            },
            Err(_) => {
                let engine = Self::new();
                let _ = engine.save(data_dir);
                engine
            }
        }
    }

    pub fn save(&self, data_dir: &str) -> Result<()> {
        let path = Self::store_path(data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.orders)?;
        let tmp = tempfile::NamedTempFile::new_in(
            path.parent().unwrap_or_else(|| std::path::Path::new(".")),
        )?;
        std::fs::write(tmp.path(), json)?;
        tmp.persist(&path)?;
        Ok(())
    }

    /// Orders whose Keyword/EveryNMessages trigger fires for this message. The
    /// caller executes each order's action (with origin gating). SessionEnd /
    /// OnBoot / Schedule are not returned here.
    pub fn matched(&self, message: &str, message_count: usize) -> Vec<StandingOrder> {
        let msg_lower = message.to_lowercase();
        self.orders
            .iter()
            .filter(|o| o.enabled)
            .filter(|o| match &o.trigger {
                OrderTrigger::Keyword { phrases } => {
                    phrases.iter().any(|p| msg_lower.contains(&p.to_lowercase()))
                }
                OrderTrigger::EveryNMessages { count } => {
                    *count > 0 && message_count > 0 && message_count % count == 0
                }
                _ => false,
            })
            .cloned()
            .collect()
    }

    /// Add a standing order
    pub fn add(&mut self, order: StandingOrder) {
        info!("Added standing order: {}", order.name);
        self.orders.push(order);
    }

    /// Remove a standing order
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.orders.len();
        self.orders.retain(|o| o.id != id);
        self.orders.len() < before
    }

    /// List all standing orders
    pub fn list(&self) -> &[StandingOrder] {
        &self.orders
    }

    /// Check orders that match a message and return context to inject
    pub fn check_message(&self, message: &str, message_count: usize) -> Vec<String> {
        let mut context_injections = Vec::new();
        let msg_lower = message.to_lowercase();

        for order in &self.orders {
            if !order.enabled { continue; }

            match &order.trigger {
                OrderTrigger::Keyword { phrases } => {
                    for phrase in phrases {
                        if msg_lower.contains(&phrase.to_lowercase()) {
                            match &order.action {
                                OrderAction::InjectContext { text } => {
                                    context_injections.push(text.clone());
                                }
                                OrderAction::SaveNote { template } => {
                                    debug!("Standing order '{}' triggered note save", order.name);
                                }
                                OrderAction::RunTool { tool_name, .. } => {
                                    debug!("Standing order '{}' triggering tool: {}", order.name, tool_name);
                                }
                                OrderAction::SendMessage { text } => {
                                    debug!("Standing order '{}' sending message: {}", order.name, text);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                OrderTrigger::EveryNMessages { count } => {
                    if message_count > 0 && message_count % count == 0 {
                        if let OrderAction::InjectContext { text } = &order.action {
                            context_injections.push(text.clone());
                        }
                    }
                }
                OrderTrigger::SessionEnd => {
                    // Handled by hooks system
                }
                OrderTrigger::OnBoot => {
                    // Handled by hooks system
                }
                _ => {}
            }
        }

        context_injections
    }

    /// Check session-end orders
    pub fn check_session_end(&self) -> Vec<&StandingOrder> {
        self.orders.iter()
            .filter(|o| o.enabled && matches!(o.trigger, OrderTrigger::SessionEnd))
            .collect()
    }

    /// Parse a standing order from natural language
    pub fn parse_from_text(input: &str) -> Option<StandingOrder> {
        // Simple parsing: "When I mention X, do Y"
        let input_lower = input.to_lowercase();

        if input_lower.starts_with("when i mention") {
            let rest = input.trim_start_matches("when i mention").trim();
            let parts: Vec<&str> = rest.split(", then ").collect();
            if parts.len() == 2 {
                let phrases: Vec<String> = parts[0].split(" or ")
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .collect();
                let action_text = parts[1].trim();

                return Some(StandingOrder {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: format!("auto-{}", phrases.join("-")),
                    enabled: true,
                    trigger: OrderTrigger::Keyword { phrases },
                    action: if action_text.starts_with("remember") {
                        OrderAction::SaveNote { template: action_text.to_string() }
                    } else if action_text.starts_with("run") {
                        let tool = action_text.trim_start_matches("run").trim();
                        OrderAction::RunTool {
                            tool_name: tool.to_string(),
                            arguments: serde_json::json!({}),
                        }
                    } else {
                        OrderAction::InjectContext { text: action_text.to_string() }
                    },
                    description: input.to_string(),
                });
            }
        }

        None
    }

    /// Load default standing orders
    fn load_defaults(&mut self) {
        self.add(StandingOrder {
            id: "remember-preferences".to_string(),
            name: "Auto-remember preferences".to_string(),
            enabled: true,
            trigger: OrderTrigger::Keyword {
                phrases: vec![
                    "i prefer".into(), "i like".into(), "i want".into(),
                    "remember that".into(), "don't forget".into(),
                    "my favorite".into(), "always".into(), "never".into(),
                ],
            },
            action: OrderAction::SaveNote {
                template: "User preference detected: {{message}}".to_string(),
            },
            description: "Automatically remember user preferences".to_string(),
        });

        self.add(StandingOrder {
            id: "session-summary".to_string(),
            name: "Session summary on end".to_string(),
            enabled: true,
            trigger: OrderTrigger::SessionEnd,
            action: OrderAction::SaveNote {
                template: "Session summary: {{message_count}} topics discussed".to_string(),
            },
            description: "Summarize session when it ends".to_string(),
        });

        self.add(StandingOrder {
            id: "project-tracker".to_string(),
            name: "Track project mentions".to_string(),
            enabled: true,
            trigger: OrderTrigger::Keyword {
                phrases: vec![
                    "working on".into(), "project".into(), "building".into(),
                    "developing".into(), "creating".into(),
                ],
            },
            action: OrderAction::SaveNote {
                template: "Project mention: {{message}}".to_string(),
            },
            description: "Track project mentions across sessions".to_string(),
        });
    }
}

/// Render a SaveNote/InjectContext template: substitutes `{{message}}` and
/// `{{message_count}}`.
pub fn render_template(template: &str, message: &str, message_count: usize) -> String {
    template
        .replace("{{message}}", message)
        .replace("{{message_count}}", &message_count.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_template_substitutes_placeholders() {
        assert_eq!(render_template("pref: {{message}}", "I like tea", 3), "pref: I like tea");
        assert_eq!(render_template("{{message_count}} msgs", "x", 7), "7 msgs");
        assert_eq!(render_template("none", "x", 1), "none");
        assert_eq!(render_template("{{message}}/{{message}}", "a", 1), "a/a");
    }

    #[test]
    fn matched_respects_triggers_and_enabled() {
        let mut e = StandingOrdersEngine::default();
        e.add(StandingOrder {
            id: "k".into(), name: "kw".into(), enabled: true,
            trigger: OrderTrigger::Keyword { phrases: vec!["Deploy".into()] },
            action: OrderAction::InjectContext { text: "ctx".into() },
            description: String::new(),
        });
        e.add(StandingOrder {
            id: "n".into(), name: "every3".into(), enabled: true,
            trigger: OrderTrigger::EveryNMessages { count: 3 },
            action: OrderAction::InjectContext { text: "tick".into() },
            description: String::new(),
        });
        e.add(StandingOrder {
            id: "d".into(), name: "disabled".into(), enabled: false,
            trigger: OrderTrigger::Keyword { phrases: vec!["deploy".into()] },
            action: OrderAction::InjectContext { text: "no".into() },
            description: String::new(),
        });
        e.add(StandingOrder {
            id: "s".into(), name: "end".into(), enabled: true,
            trigger: OrderTrigger::SessionEnd,
            action: OrderAction::SaveNote { template: "t".into() },
            description: String::new(),
        });

        // Keyword is case-insensitive; disabled order excluded; SessionEnd never matched here.
        let m = e.matched("please DEPLOY now", 1);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "k");
        // EveryNMessages: fires at multiples of 3.
        assert!(e.matched("no keyword", 3).iter().any(|o| o.id == "n"));
        assert!(!e.matched("no keyword", 4).iter().any(|o| o.id == "n"));
        // SessionEnd surfaced only via check_session_end.
        assert_eq!(e.check_session_end().len(), 1);
    }

    #[test]
    fn load_seeds_defaults_then_round_trips_custom() {
        let dir = tempfile::tempdir().unwrap();
        let dd = dir.path().to_str().unwrap();
        // First load with no file → seeds defaults and writes the file.
        let e1 = StandingOrdersEngine::load(dd);
        assert!(!e1.list().is_empty(), "defaults seeded");
        assert!(dir.path().join("standing_orders.json").exists(), "persisted");

        // Save a custom set, reload, confirm it round-trips.
        let mut e2 = StandingOrdersEngine::default();
        e2.add(StandingOrder {
            id: "only".into(), name: "only".into(), enabled: true,
            trigger: OrderTrigger::Keyword { phrases: vec!["x".into()] },
            action: OrderAction::InjectContext { text: "y".into() },
            description: String::new(),
        });
        e2.save(dd).unwrap();
        let e3 = StandingOrdersEngine::load(dd);
        assert_eq!(e3.list().len(), 1);
        assert_eq!(e3.list()[0].id, "only");
    }
}

/// Task flow — a sequence of steps to accomplish a multi-step task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFlow {
    pub id: String,
    pub name: String,
    pub steps: Vec<TaskStep>,
    pub current_step: usize,
    pub status: TaskFlowStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    pub name: String,
    pub description: String,
    pub tool: Option<String>,
    pub arguments: Option<serde_json::Value>,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskFlowStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Default)]
pub struct TaskFlowEngine {
    flows: Vec<TaskFlow>,
}

impl TaskFlow {
    pub fn create_flow(name: &str, steps: Vec<TaskStep>) -> TaskFlow {
        TaskFlow {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            steps,
            current_step: 0,
            status: TaskFlowStatus::Pending,
        }
    }
}

impl TaskFlowEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_flow(name: &str, steps: Vec<TaskStep>) -> TaskFlow {
        TaskFlow {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            steps,
            current_step: 0,
            status: TaskFlowStatus::Pending,
        }
    }

    pub fn add_flow(&mut self, flow: TaskFlow) {
        self.flows.push(flow);
    }

    pub fn get_flow(&self, id: &str) -> Option<&TaskFlow> {
        self.flows.iter().find(|f| f.id == id)
    }

    pub fn advance(&mut self, id: &str) -> Option<&TaskStep> {
        if let Some(flow) = self.flows.iter_mut().find(|f| f.id == id) {
            if flow.current_step < flow.steps.len() {
                flow.steps[flow.current_step].completed = true;
                flow.current_step += 1;
                if flow.current_step >= flow.steps.len() {
                    flow.status = TaskFlowStatus::Completed;
                } else {
                    flow.status = TaskFlowStatus::Running;
                }
                return flow.steps.get(flow.current_step);
            }
        }
        None
    }

    pub fn list_active(&self) -> Vec<&TaskFlow> {
        self.flows.iter()
            .filter(|f| f.status == TaskFlowStatus::Running || f.status == TaskFlowStatus::Pending)
            .collect()
    }
}

/// Built-in task flows
pub fn built_in_flows() -> Vec<TaskFlow> {
    vec![
        TaskFlow::create_flow("daily-briefing", vec![
            TaskStep { name: "Check calendar".into(), description: "Review today's calendar".into(), tool: Some("calendar".into()), arguments: None, completed: false },
            TaskStep { name: "Check email".into(), description: "Scan unread emails for urgent items".into(), tool: Some("email".into()), arguments: None, completed: false },
            TaskStep { name: "Weather".into(), description: "Check weather for today".into(), tool: Some("web_search".into()), arguments: Some(serde_json::json!({"query": "weather today"})), completed: false },
            TaskStep { name: "Summarize".into(), description: "Compile daily briefing".into(), tool: None, arguments: None, completed: false },
        ]),
        TaskFlow::create_flow("code-review", vec![
            TaskStep { name: "Fetch diff".into(), description: "Get the code diff".into(), tool: Some("shell".into()), arguments: None, completed: false },
            TaskStep { name: "Analyze".into(), description: "Review code for issues".into(), tool: None, arguments: None, completed: false },
            TaskStep { name: "Suggest".into(), description: "Provide improvement suggestions".into(), tool: None, arguments: None, completed: false },
        ]),
    ]
}
