// src/core/goal_system.rs
//! Goal/Planning system — multi-agent deliberation + Claude Code features
//!
//! Multi-Agent Goal Deliberation:
//! - Judge: unbiased evaluation of goal feasibility and quality
//! - Devil's Advocate: challenges assumptions, finds weaknesses
//! - Researcher: gathers context, finds relevant information
//! - Executor: plans concrete implementation steps
//! - Synthesizer: combines all perspectives into final recommendation
//!
//! Claude Code features:
//! - TodoWrite: persistent task list across turns
//! - Plan Mode: read-only planning vs read-write execution
//! - Context compression: automatic summarization
//! - Permission system: allow/deny/ask per tool

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, debug};

// ─── Plan Mode (Claude Code-style) ───────────────────────────────────

/// Whether the agent is in read-only plan mode or read-write execute mode
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PlanMode {
    /// Agent can read files and explore but NOT modify anything
    Plan,
    /// Agent can read AND write files, run commands, etc.
    Execute,
}

impl Default for PlanMode {
    fn default() -> Self {
        PlanMode::Execute
    }
}

impl PlanMode {
    pub fn toggle(&mut self) {
        *self = match self {
            PlanMode::Plan => PlanMode::Execute,
            PlanMode::Execute => PlanMode::Plan,
        };
        info!("Plan mode toggled to: {:?}", self);
    }

    pub fn can_write(&self) -> bool {
        matches!(self, PlanMode::Execute)
    }

    pub fn can_execute(&self) -> bool {
        matches!(self, PlanMode::Execute)
    }

    pub fn label(&self) -> &str {
        match self {
            PlanMode::Plan => "📋 PLAN MODE (read-only)",
            PlanMode::Execute => "⚡ EXECUTE MODE (read-write)",
        }
    }
}

// ─── Permission System (Claude Code-style) ───────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Permission {
    Allow,
    Deny,
    Ask,
}

impl Default for Permission {
    fn default() -> Self {
        Permission::Ask
    }
}

impl Permission {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "allow" | "yes" | "true" | "1" => Permission::Allow,
            "deny" | "no" | "false" | "0" => Permission::Deny,
            _ => Permission::Ask,
        }
    }

    pub fn is_allowed(&self) -> bool {
        matches!(self, Permission::Allow)
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Permission::Deny)
    }

    pub fn needs_ask(&self) -> bool {
        matches!(self, Permission::Ask)
    }
}

/// Permission manager — per-tool permission settings
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PermissionManager {
    permissions: HashMap<String, Permission>,
    global_default: Permission,
}

impl PermissionManager {
    pub fn new() -> Self {
        let mut mgr = Self {
            permissions: HashMap::new(),
            global_default: Permission::Ask,
        };
        // Safe defaults
        mgr.set("read", Permission::Allow);
        mgr.set("glob", Permission::Allow);
        mgr.set("grep", Permission::Allow);
        mgr.set("todo_write", Permission::Allow);
        mgr.set("memory", Permission::Allow);
        mgr
    }

    pub fn set(&mut self, tool: &str, perm: Permission) {
        self.permissions.insert(tool.to_string(), perm);
    }

    pub fn get(&self, tool: &str) -> Permission {
        self.permissions.get(tool).copied().unwrap_or(self.global_default)
    }

    pub fn check(&self, tool: &str) -> PermissionResult {
        match self.get(tool) {
            Permission::Allow => PermissionResult::Allowed,
            Permission::Deny => PermissionResult::Denied(format!("Tool '{}' is denied by permission policy", tool)),
            Permission::Ask => PermissionResult::NeedsApproval(format!("Tool '{}' requires approval", tool)),
        }
    }

    pub fn format_permissions(&self) -> String {
        let mut output = String::from("🔐 Permission Settings:\n");
        let mut tools: Vec<_> = self.permissions.iter().collect();
        tools.sort_by_key(|(k, _)| *k);
        for (tool, perm) in tools {
            let icon = match perm {
                Permission::Allow => "✅",
                Permission::Deny => "❌",
                Permission::Ask => "❓",
            };
            output.push_str(&format!("  {} {}: {:?}\n", icon, tool, perm));
        }
        output
    }
}

#[derive(Debug, Clone)]
pub enum PermissionResult {
    Allowed,
    Denied(String),
    NeedsApproval(String),
}

// ─── TodoWrite (Claude Code-style persistent task list) ──────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: Priority,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Persistent task list — survives across conversation turns (Claude Code TodoWrite)
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TodoList {
    todos: Vec<TodoItem>,
    max_todos: usize,
}

impl TodoList {
    pub fn new() -> Self {
        Self {
            todos: Vec::new(),
            max_todos: 20,
        }
    }

    /// Replace the entire todo list (TodoWrite tool behavior)
    pub fn replace(&mut self, items: Vec<(String, String, String)>) -> Vec<String> {
        self.todos.clear();
        let mut ids = Vec::new();
        for (content, status, priority) in items {
            if self.todos.len() >= self.max_todos {
                break;
            }
            let id = format!("todo_{}", self.todos.len() + 1);
            let status = match status.as_str() {
                "in_progress" => TodoStatus::InProgress,
                "completed" => TodoStatus::Completed,
                "cancelled" => TodoStatus::Cancelled,
                _ => TodoStatus::Pending,
            };
            let priority = match priority.as_str() {
                "high" => Priority::High,
                "low" => Priority::Low,
                _ => Priority::Medium,
            };
            self.todos.push(TodoItem {
                id: id.clone(),
                content,
                status,
                priority,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
            ids.push(id);
        }
        info!("TodoWrite: replaced with {} items", self.todos.len());
        ids
    }

    /// Update a single todo item
    pub fn update_status(&mut self, id: &str, status: TodoStatus) -> bool {
        if let Some(todo) = self.todos.iter_mut().find(|t| t.id == id) {
            todo.status = status;
            todo.updated_at = chrono::Utc::now();
            true
        } else {
            false
        }
    }

    /// Mark a todo as in_progress (only one at a time)
    pub fn start_task(&mut self, id: &str) -> bool {
        // First, mark any in_progress tasks as pending
        for todo in &mut self.todos {
            if todo.status == TodoStatus::InProgress {
                todo.status = TodoStatus::Pending;
            }
        }
        self.update_status(id, TodoStatus::InProgress)
    }

    pub fn complete_task(&mut self, id: &str) -> bool {
        self.update_status(id, TodoStatus::Completed)
    }

    pub fn list(&self) -> &[TodoItem] {
        &self.todos
    }

    pub fn progress(&self) -> (usize, usize) {
        let total = self.todos.len();
        let completed = self.todos.iter().filter(|t| t.status == TodoStatus::Completed).count();
        (completed, total)
    }

    pub fn format_todos(&self) -> String {
        let mut output = String::new();
        let (completed, total) = self.progress();
        output.push_str(&format!("📋 Todo List: {}/{} completed\n", completed, total));
        output.push_str(&"─".repeat(50));
        output.push('\n');

        for todo in &self.todos {
            let status_icon = match todo.status {
                TodoStatus::Pending => "⬜",
                TodoStatus::InProgress => "🔄",
                TodoStatus::Completed => "✅",
                TodoStatus::Cancelled => "❌",
            };
            let priority_icon = match todo.priority {
                Priority::Critical => "🔴",
                Priority::High => "🟠",
                Priority::Medium => "🟡",
                Priority::Low => "🟢",
            };
            output.push_str(&format!("  {} {} [{}] {}\n", status_icon, priority_icon, &todo.id, &todo.content));
        }
        output
    }
}

// ─── Task Management (Hermes-style) ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Blocked,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Medium
    }
}

/// A single task with subtasks, status, and priority
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub subtasks: Vec<Subtask>,
    pub parent_id: Option<String>,
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub id: String,
    pub title: String,
    pub completed: bool,
}

/// A goal — a high-level objective decomposed into subgoals (Hermes-style).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
    pub subgoals: Vec<Subgoal>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub target_date: Option<chrono::DateTime<chrono::Utc>>,
}

impl Goal {
    /// (completed, total) subgoals — the progress rollup for this goal.
    pub fn progress(&self) -> (usize, usize) {
        let total = self.subgoals.len();
        let completed = self.subgoals.iter().filter(|s| s.completed).count();
        (completed, total)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalStatus {
    Draft,
    Active,
    Paused,
    Completed,
    Abandoned,
}

/// A subgoal — an intermediate objective under a goal, decomposed into tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subgoal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub tasks: Vec<String>, // Task IDs
}

/// Task board — manages goals, subgoals, and tasks. Serializable so it can be
/// persisted (see `goal_store::GoalStore`).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TaskBoard {
    tasks: HashMap<String, Task>,
    goals: HashMap<String, Goal>,
}

impl TaskBoard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_task(&mut self, title: &str, description: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let task = Task {
            id: id.clone(),
            title: title.to_string(),
            description: description.to_string(),
            status: TaskStatus::Pending,
            priority: Priority::Medium,
            subtasks: Vec::new(),
            parent_id: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
            notes: Vec::new(),
        };
        info!("Created task: {} — {}", &id[..8], title);
        self.tasks.insert(id.clone(), task);
        id
    }

    pub fn add_subtask(&mut self, task_id: &str, title: &str) -> Option<String> {
        if let Some(task) = self.tasks.get_mut(task_id) {
            let st_id = uuid::Uuid::new_v4().to_string();
            task.subtasks.push(Subtask {
                id: st_id.clone(),
                title: title.to_string(),
                completed: false,
            });
            task.updated_at = chrono::Utc::now();
            return Some(st_id);
        }
        None
    }

    pub fn complete_subtask(&mut self, task_id: &str, subtask_id: &str) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            for st in &mut task.subtasks {
                if st.id == subtask_id {
                    st.completed = true;
                    task.updated_at = chrono::Utc::now();
                    if task.subtasks.iter().all(|s| s.completed) {
                        task.status = TaskStatus::Completed;
                        task.completed_at = Some(chrono::Utc::now());
                    }
                    return true;
                }
            }
        }
        false
    }

    pub fn set_task_status(&mut self, task_id: &str, status: TaskStatus) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = status.clone();
            task.updated_at = chrono::Utc::now();
            if status == TaskStatus::Completed {
                task.completed_at = Some(chrono::Utc::now());
            }
            return true;
        }
        false
    }

    pub fn set_task_priority(&mut self, task_id: &str, priority: Priority) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.priority = priority;
            task.updated_at = chrono::Utc::now();
            return true;
        }
        false
    }

    pub fn add_task_note(&mut self, task_id: &str, note: &str) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.notes.push(note.to_string());
            task.updated_at = chrono::Utc::now();
            return true;
        }
        false
    }

    pub fn get_task(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    pub fn list_tasks(&self, filter: Option<TaskStatus>) -> Vec<&Task> {
        self.tasks.values()
            .filter(|t| filter.as_ref().map_or(true, |f| t.status == *f))
            .collect()
    }

    pub fn list_by_priority(&self) -> Vec<&Task> {
        let mut tasks: Vec<&Task> = self.tasks.values()
            .filter(|t| t.status != TaskStatus::Completed && t.status != TaskStatus::Cancelled)
            .collect();
        tasks.sort_by_key(|t| match t.priority {
            Priority::Critical => 0,
            Priority::High => 1,
            Priority::Medium => 2,
            Priority::Low => 3,
        });
        tasks
    }

    pub fn progress(&self) -> (usize, usize) {
        let total = self.tasks.len();
        let completed = self.tasks.values().filter(|t| t.status == TaskStatus::Completed).count();
        (completed, total)
    }

    pub fn create_goal(&mut self, title: &str, description: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let goal = Goal {
            id: id.clone(),
            title: title.to_string(),
            description: description.to_string(),
            status: GoalStatus::Active,
            subgoals: Vec::new(),
            created_at: chrono::Utc::now(),
            target_date: None,
        };
        info!("Created goal: {} — {}", &id[..8], title);
        self.goals.insert(id.clone(), goal);
        id
    }

    pub fn create_subgoal(&mut self, goal_id: &str, title: &str, description: &str) -> Option<String> {
        if let Some(goal) = self.goals.get_mut(goal_id) {
            let sg_id = uuid::Uuid::new_v4().to_string();
            goal.subgoals.push(Subgoal {
                id: sg_id.clone(),
                title: title.to_string(),
                description: description.to_string(),
                completed: false,
                tasks: Vec::new(),
            });
            return Some(sg_id);
        }
        None
    }

    pub fn complete_subgoal(&mut self, goal_id: &str, subgoal_id: &str) -> bool {
        if let Some(goal) = self.goals.get_mut(goal_id) {
            for sg in &mut goal.subgoals {
                if sg.id == subgoal_id {
                    sg.completed = true;
                    return true;
                }
            }
        }
        false
    }

    /// Create a task and attach it to a subgoal. Returns the new task id, or
    /// `None` if the goal/subgoal does not exist (no orphan task is created).
    pub fn add_task_to_subgoal(&mut self, goal_id: &str, subgoal_id: &str, title: &str) -> Option<String> {
        let exists = self
            .goals
            .get(goal_id)
            .map_or(false, |g| g.subgoals.iter().any(|s| s.id == subgoal_id));
        if !exists {
            return None;
        }
        let task_id = self.create_task(title, "");
        if let Some(goal) = self.goals.get_mut(goal_id) {
            if let Some(sg) = goal.subgoals.iter_mut().find(|s| s.id == subgoal_id) {
                sg.tasks.push(task_id.clone());
            }
        }
        Some(task_id)
    }

    pub fn goal_progress(&self, goal_id: &str) -> (usize, usize) {
        self.goals.get(goal_id).map(|g| g.progress()).unwrap_or((0, 0))
    }

    pub fn get_goal(&self, goal_id: &str) -> Option<&Goal> {
        self.goals.get(goal_id)
    }

    pub fn list_goals(&self) -> Vec<&Goal> {
        self.goals.values().collect()
    }

    pub fn format_board(&self) -> String {
        let mut output = String::new();
        let (completed, total) = self.progress();
        output.push_str(&format!("📊 Task Board: {}/{} completed\n", completed, total));
        output.push_str(&"─".repeat(50));
        output.push('\n');

        let by_priority = self.list_by_priority();
        if !by_priority.is_empty() {
            output.push_str("\n📋 Active Tasks (by priority):\n");
            for task in by_priority {
                let status_icon = match task.status {
                    TaskStatus::Pending => "⬜",
                    TaskStatus::InProgress => "🔄",
                    TaskStatus::Blocked => "🚫",
                    TaskStatus::Completed => "✅",
                    TaskStatus::Cancelled => "❌",
                };
                let priority_icon = match task.priority {
                    Priority::Critical => "🔴",
                    Priority::High => "🟠",
                    Priority::Medium => "🟡",
                    Priority::Low => "🟢",
                };
                output.push_str(&format!("  {} {} {} {}\n", status_icon, priority_icon,
                    &task.title, if !task.subtasks.is_empty() {
                        format!("({}/{} subtasks)", task.subtasks.iter().filter(|s| s.completed).count(), task.subtasks.len())
                    } else { String::new() }
                ));
            }
        }

        let goals = self.list_goals();
        if !goals.is_empty() {
            output.push_str("\n🎯 Goals:\n");
            for goal in goals {
                let status_icon = match goal.status {
                    GoalStatus::Draft => "📝",
                    GoalStatus::Active => "🏃",
                    GoalStatus::Paused => "⏸️",
                    GoalStatus::Completed => "🎉",
                    GoalStatus::Abandoned => "🚫",
                };
                let (sg_done, sg_total) = self.goal_progress(&goal.id);
                output.push_str(&format!("  {} {} ({}/{} subgoals)\n", status_icon, &goal.title, sg_done, sg_total));
            }
        }

        output
    }
}

// ─── Multi-Agent Goal Deliberation ───────────────────────────────────

/// Agent roles for goal deliberation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash, Eq)]
pub enum DeliberationRole {
    /// Unbiased judge — evaluates feasibility, quality, completeness
    Judge,
    /// Devil's Advocate — challenges assumptions, finds weaknesses
    DevilsAdvocate,
    /// Researcher — gathers context, finds relevant information
    Researcher,
    /// Executor — plans concrete implementation steps
    Executor,
    /// Synthesizer — combines all perspectives into final recommendation
    Synthesizer,
    /// Domain Expert — provides domain-specific knowledge (added dynamically)
    DomainExpert(String),
}

impl DeliberationRole {
    pub fn name(&self) -> &str {
        match self {
            DeliberationRole::Judge => "Judge",
            DeliberationRole::DevilsAdvocate => "Devil's Advocate",
            DeliberationRole::Researcher => "Researcher",
            DeliberationRole::Executor => "Executor",
            DeliberationRole::Synthesizer => "Synthesizer",
            DeliberationRole::DomainExpert(domain) => domain,
        }
    }

    pub fn emoji(&self) -> &str {
        match self {
            DeliberationRole::Judge => "⚖️",
            DeliberationRole::DevilsAdvocate => "😈",
            DeliberationRole::Researcher => "🔍",
            DeliberationRole::Executor => "🔧",
            DeliberationRole::Synthesizer => "🧩",
            DeliberationRole::DomainExpert(_) => "🎓",
        }
    }

    /// Get the system prompt for this role
    pub fn system_prompt(&self) -> &str {
        match self {
            DeliberationRole::Judge => "\
                You are an unbiased Judge evaluating a goal or plan. Your role is to:
                1. Assess feasibility — is this realistic given constraints?
                2. Evaluate quality — is this well-defined and complete?
                3. Check alignment — does this match the user's stated needs?
                4. Rate confidence — how confident are you this will succeed?
                Be fair, balanced, and evidence-based. Give a score from 1-10 and explain your reasoning.",

            DeliberationRole::DevilsAdvocate => "\
                You are the Devil's Advocate. Your role is to challenge everything:
                1. Find weaknesses in the plan — what could go wrong?
                2. Challenge assumptions — what are we taking for granted?
                3. Identify risks — what are the failure modes?
                4. Question feasibility — is this really doable?
                Be constructively critical. Your job is to stress-test the plan, not to be negative for its own sake.",

            DeliberationRole::Researcher => "\
                You are the Researcher. Your role is to gather and present relevant context:
                1. What similar approaches exist? What can we learn from them?
                2. What are the best practices in this domain?
                3. What tools, libraries, or resources are available?
                4. What are the common pitfalls and how to avoid them?
                Provide concrete, actionable information.",

            DeliberationRole::Executor => "\
                You are the Executor. Your role is to plan concrete implementation:
                1. Break the goal into specific, actionable steps
                2. Estimate effort and time for each step
                3. Identify dependencies between steps
                4. Suggest the optimal order of execution
                Be practical and specific. Think about what actually needs to happen.",

            DeliberationRole::Synthesizer => "\
                You are the Synthesizer. Your role is to combine all perspectives:
                1. Review all agent inputs and identify areas of agreement
                2. Resolve conflicts between different viewpoints
                3. Create a final, balanced recommendation
                4. Highlight the strongest points from each perspective
                Produce a clear, actionable final plan that incorporates the best insights.",

            DeliberationRole::DomainExpert(_) => "\
                You are a Domain Expert. Your role is to provide specialized knowledge:
                1. Share domain-specific insights and best practices
                2. Identify domain-specific risks and opportunities
                3. Suggest domain-specific tools and approaches
                4. Validate technical feasibility in your domain",
        }
    }
}

/// A single agent's contribution to the deliberation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliberationContribution {
    pub role: DeliberationRole,
    pub content: String,
    pub confidence: Option<f32>, // 0.0 - 1.0
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// The full deliberation process for a goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalDeliberation {
    pub goal_id: String,
    pub goal_title: String,
    pub goal_description: String,
    pub contributions: Vec<DeliberationContribution>,
    pub final_recommendation: Option<String>,
    pub status: DeliberationStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeliberationStatus {
    InProgress,
    AwaitingSynthesis,
    Completed,
}

/// Multi-Agent Goal Deliberation orchestrator
#[derive(Debug)]
pub struct GoalDeliberator {
    active_deliberations: HashMap<String, GoalDeliberation>,
    /// Which roles to use for deliberation (configurable)
    default_roles: Vec<DeliberationRole>,
}

impl Default for GoalDeliberator {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalDeliberator {
    pub fn new() -> Self {
        Self {
            active_deliberations: HashMap::new(),
            default_roles: vec![
                DeliberationRole::Judge,
                DeliberationRole::DevilsAdvocate,
                DeliberationRole::Researcher,
                DeliberationRole::Executor,
                DeliberationRole::Synthesizer,
            ],
        }
    }

    /// Start a new deliberation for a goal
    pub fn start_deliberation(&mut self, goal_id: &str, title: &str, description: &str) -> &GoalDeliberation {
        let deliberation = GoalDeliberation {
            goal_id: goal_id.to_string(),
            goal_title: title.to_string(),
            goal_description: description.to_string(),
            contributions: Vec::new(),
            final_recommendation: None,
            status: DeliberationStatus::InProgress,
            created_at: chrono::Utc::now(),
            completed_at: None,
        };
        info!("Started deliberation for goal: {}", title);
        self.active_deliberations.insert(goal_id.to_string(), deliberation);
        self.active_deliberations.get(goal_id).unwrap()
    }

    /// Add a contribution from a specific role
    pub fn add_contribution(&mut self, goal_id: &str, role: DeliberationRole, content: &str, confidence: Option<f32>) -> bool {
        if let Some(deliberation) = self.active_deliberations.get_mut(goal_id) {
            deliberation.contributions.push(DeliberationContribution {
                role,
                content: content.to_string(),
                confidence,
                timestamp: chrono::Utc::now(),
            });
            true
        } else {
            false
        }
    }

    /// Check if all required roles have contributed
    pub fn is_ready_for_synthesis(&self, goal_id: &str) -> bool {
        if let Some(deliberation) = self.active_deliberations.get(goal_id) {
            let contributed_roles: std::collections::HashSet<_> = deliberation.contributions.iter()
                .map(|c| c.role.name().to_string())
                .collect();
            // Need at least Judge, Devil's Advocate, and Researcher before synthesis
            contributed_roles.contains("Judge")
                && contributed_roles.contains("Devil's Advocate")
                && contributed_roles.contains("Researcher")
        } else {
            false
        }
    }

    /// Set the final synthesized recommendation
    pub fn set_synthesis(&mut self, goal_id: &str, recommendation: &str) -> bool {
        if let Some(deliberation) = self.active_deliberations.get_mut(goal_id) {
            deliberation.final_recommendation = Some(recommendation.to_string());
            deliberation.status = DeliberationStatus::Completed;
            deliberation.completed_at = Some(chrono::Utc::now());
            true
        } else {
            false
        }
    }

    /// Get the default roles for deliberation
    pub fn default_roles(&self) -> &[DeliberationRole] {
        &self.default_roles
    }

    /// Add a custom role (e.g., Domain Expert for specific topics)
    pub fn add_role(&mut self, role: DeliberationRole) {
        if !self.default_roles.contains(&role) {
            self.default_roles.push(role);
        }
    }

    /// Format the full deliberation for display
    pub fn format_deliberation(&self, goal_id: &str) -> String {
        if let Some(d) = self.active_deliberations.get(goal_id) {
            let mut output = String::new();
            output.push_str(&format!("🎯 Goal Deliberation: {}\n", d.goal_title));
            output.push_str(&format!("📝 {}\n", d.goal_description));
            output.push_str(&format!("Status: {:?}\n", d.status));
            output.push_str(&"─".repeat(60));
            output.push('\n');

            for contrib in &d.contributions {
                output.push_str(&format!("\n{} {} (confidence: {:?}):\n",
                    contrib.role.emoji(), contrib.role.name(), contrib.confidence));
                output.push_str(&format!("{}\n", contrib.content));
            }

            if let Some(ref rec) = d.final_recommendation {
                output.push_str(&format!("\n{}\n", "═".repeat(60)));
                output.push_str("🧩 Final Synthesized Recommendation:\n");
                output.push_str(rec);
                output.push('\n');
            }

            output
        } else {
            "No deliberation found for this goal.".to_string()
        }
    }

    /// Build the prompt for a specific role to contribute
    pub fn build_role_prompt(&self, goal_id: &str, role: &DeliberationRole) -> Option<String> {
        let deliberation = self.active_deliberations.get(goal_id)?;

        let mut prompt = String::new();
        prompt.push_str(role.system_prompt());
        push_str(&mut prompt, &format!("\n## Goal: {}", deliberation.goal_title));
        push_str(&mut prompt, &format!("Description: {}", deliberation.goal_description));

        // Include previous contributions for context
        if !deliberation.contributions.is_empty() {
            push_str(&mut prompt, "\n## Previous Contributions:");
            for contrib in &deliberation.contributions {
                push_str(&mut prompt, &format!("\n### {}:", contrib.role.name()));
                // Truncate long contributions
                let truncated = if contrib.content.len() > 500 {
                    format!("{}...", &contrib.content[..500])
                } else {
                    contrib.content.clone()
                };
                push_str(&mut prompt, &truncated);
            }
        }

        push_str(&mut prompt, &format!("\n\nProvide your {} analysis. Be specific and actionable.", role.name()));

        Some(prompt)
    }
}

// ─── Context Files (Claude Code CLAUDE.md style) ─────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFile {
    pub path: PathBuf,
    pub content: String,
    pub file_type: ContextFileType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContextFileType {
    ProjectContext,
    UserContext,
    AgentConfig,
    DirectoryContext,
}

#[derive(Debug, Default)]
pub struct ContextFileManager {
    files: HashMap<PathBuf, ContextFile>,
}

impl ContextFileManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_project_context(&mut self, workspace_dir: &str) -> Option<&ContextFile> {
        let path = PathBuf::from(format!("{}/CLAUDE.md", workspace_dir));
        if !self.files.contains_key(&path) {
            let default_content = "# Project Context\n\n## Instructions\n- Be helpful and concise\n- Ask before making destructive changes\n- Prefer idiomatic code\n\n## Project Structure\n[Describe your project structure here]\n\n## Coding Standards\n[Define your coding standards here]\n\n## Notes\n[Any other notes for the agent]\n".to_string();
            self.files.insert(path.clone(), ContextFile {
                path: path.clone(),
                content: default_content,
                file_type: ContextFileType::ProjectContext,
            });
        }
        self.files.get(&path)
    }

    pub fn find_context_file(&self, dir: &str, file_type: &ContextFileType) -> Option<&ContextFile> {
        let mut current = PathBuf::from(dir);
        loop {
            for (path, file) in &self.files {
                if path.parent() == Some(&current) && &file.file_type == file_type {
                    return Some(file);
                }
            }
            if !current.pop() {
                break;
            }
        }
        None
    }

    pub fn set_context(&mut self, path: PathBuf, content: &str, file_type: ContextFileType) {
        self.files.insert(path.clone(), ContextFile {
            path,
            content: content.to_string(),
            file_type,
        });
    }

    pub fn build_full_context(&self, workspace_dir: &str) -> String {
        let mut ctx = String::new();

        if let Some(user) = self.find_context_file(workspace_dir, &ContextFileType::UserContext) {
            ctx.push_str("# User Instructions\n");
            ctx.push_str(&user.content);
            ctx.push_str("\n\n");
        }

        if let Some(project) = self.find_context_file(workspace_dir, &ContextFileType::ProjectContext) {
            ctx.push_str("# Project Context\n");
            ctx.push_str(&project.content);
            ctx.push_str("\n\n");
        }

        if let Some(agent) = self.find_context_file(workspace_dir, &ContextFileType::AgentConfig) {
            ctx.push_str("# Agent Configuration\n");
            ctx.push_str(&agent.content);
            ctx.push('\n');
        }

        ctx
    }

    pub fn save_to_disk(&self, workspace_dir: &str) -> Result<()> {
        for (path, file) in &self.files {
            if path.parent() == Some(&PathBuf::from(workspace_dir)) {
                std::fs::write(path, &file.content)?;
                debug!("Saved context file: {:?}", path);
            }
        }
        Ok(())
    }
}

// ─── Session Resume (Hermes-style) ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub channel: String,
    pub user_id: String,
    pub message_count: usize,
    pub topics: Vec<String>,
    pub summary: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: Vec<SessionSummary>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_session(&mut self, summary: SessionSummary) {
        info!("Session recorded: {} ({} messages)", &summary.session_id[..8], summary.message_count);
        self.sessions.push(summary);
    }

    pub fn get_last_session(&self, channel: &str, user_id: &str) -> Option<&SessionSummary> {
        self.sessions.iter()
            .rev()
            .find(|s| s.channel == channel && s.user_id == user_id)
    }

    pub fn get_recent_sessions(&self, count: usize) -> &[SessionSummary] {
        let start = self.sessions.len().saturating_sub(count);
        &self.sessions[start..]
    }

    pub fn format_resume_summary(&self, channel: &str, user_id: &str) -> String {
        if let Some(session) = self.get_last_session(channel, user_id) {
            format!(
                "# Previous Session Summary\nSession: {} ({} messages)\nDate: {}\nTopics: {}\nSummary: {}\n",
                &session.session_id[..8],
                session.message_count,
                session.ended_at.format("%Y-%m-%d %H:%M"),
                session.topics.join(", "),
                session.summary
            )
        } else {
            String::new()
        }
    }
}

// ─── Context Compression (Claude Code /compress style) ────────────────

pub struct ContextCompressor;

impl ContextCompressor {
    pub fn compress(messages: &[super::Message], max_messages: usize) -> Vec<super::Message> {
        if messages.len() <= max_messages {
            return messages.to_vec();
        }

        let mut compressed = Vec::new();
        let keep_first = 3;
        for msg in messages.iter().take(keep_first) {
            compressed.push(msg.clone());
        }

        let summary = format!(
            "[... {} earlier messages compressed ...]\nKey topics discussed: [This would contain an LLM-generated summary]",
            messages.len() - max_messages
        );
        compressed.push(super::Message::system(&summary));

        let keep_last = max_messages - keep_first - 1;
        for msg in messages.iter().rev().take(keep_last).rev() {
            compressed.push(msg.clone());
        }

        compressed
    }
}

// ─── System Reminder Injection (Claude Code-style) ────────────────────

/// System reminders injected mid-conversation for context awareness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemReminder {
    pub content: String,
    pub priority: ReminderPriority,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReminderPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Default)]
pub struct ReminderManager {
    reminders: Vec<SystemReminder>,
}

impl ReminderManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, content: &str, priority: ReminderPriority, source: &str) {
        self.reminders.push(SystemReminder {
            content: content.to_string(),
            priority,
            source: source.to_string(),
        });
    }

    /// Get active reminders formatted for injection into system prompt
    pub fn get_active(&self) -> Vec<&SystemReminder> {
        self.reminders.iter().collect()
    }

    pub fn format_reminders(&self) -> String {
        if self.reminders.is_empty() {
            return String::new();
        }
        let mut output = String::from("⚡ System Reminders:\n");
        for reminder in &self.reminders {
            let icon = match reminder.priority {
                ReminderPriority::Low => "ℹ️",
                ReminderPriority::Medium => "⚠️",
                ReminderPriority::High => "🔶",
                ReminderPriority::Critical => "🔴",
            };
            output.push_str(&format!("  {} [{}] {}\n", icon, reminder.source, reminder.content));
        }
        output
    }

    pub fn clear(&mut self) {
        self.reminders.clear();
    }

    pub fn clear_by_source(&mut self, source: &str) {
        self.reminders.retain(|r| r.source != source);
    }
}

fn push_str(buf: &mut String, s: &str) {
    buf.push_str(s);
    buf.push('\n');
}
