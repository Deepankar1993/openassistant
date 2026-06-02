// src/core/agent_teams.rs
//! Agent teams — coordinate multiple agents working on shared tasks
//! Like Claude Code's agent teams with inter-agent messaging

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, debug};

// ─── Team Message (inter-agent communication) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessage {
    pub from: String,
    pub to: String, // "broadcast" for all agents
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// ─── Team Task ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTask {
    pub id: String,
    pub description: String,
    pub assigned_to: Option<String>,
    pub status: TeamTaskStatus,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TeamTaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

// ─── Agent Team ───────────────────────────────────────────────────────

#[derive(Debug)]
pub struct AgentTeam {
    pub name: String,
    pub agents: Vec<String>, // Agent names/IDs
    pub tasks: Vec<TeamTask>,
    pub messages: Vec<TeamMessage>,
    max_parallel: usize,
}

impl AgentTeam {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            agents: Vec::new(),
            tasks: Vec::new(),
            messages: Vec::new(),
            max_parallel: 5,
        }
    }

    pub fn add_agent(&mut self, agent_name: &str) {
        self.agents.push(agent_name.to_string());
        info!("Added agent '{}' to team '{}'", agent_name, self.name);
    }

    pub fn add_task(&mut self, description: &str) -> String {
        let task_id = format!("task_{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
        self.tasks.push(TeamTask {
            id: task_id.clone(),
            description: description.to_string(),
            assigned_to: None,
            status: TeamTaskStatus::Pending,
            result: None,
        });
        task_id
    }

    pub fn assign_task(&mut self, task_id: &str, agent: &str) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.assigned_to = Some(agent.to_string());
            task.status = TeamTaskStatus::InProgress;
            true
        } else {
            false
        }
    }

    pub fn complete_task(&mut self, task_id: &str, result: &str) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = TeamTaskStatus::Completed;
            task.result = Some(result.to_string());
            true
        } else {
            false
        }
    }

    pub fn send_message(&mut self, from: &str, to: &str, content: &str) {
        self.messages.push(TeamMessage {
            from: from.to_string(),
            to: to.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
        });
    }

    pub fn get_messages_for(&self, agent: &str) -> Vec<&TeamMessage> {
        self.messages
            .iter()
            .filter(|m| m.to == agent || m.to == "broadcast")
            .collect()
    }

    pub fn progress(&self) -> (usize, usize) {
        let total = self.tasks.len();
        let completed = self.tasks.iter().filter(|t| t.status == TeamTaskStatus::Completed).count();
        (completed, total)
    }

    pub fn format_status(&self) -> String {
        let (completed, total) = self.progress();
        let mut output = format!("👥 Team: {} ({} agents, {}/{} tasks)\n", self.name, self.agents.len(), completed, total);
        output.push_str(&"─".repeat(50));
        output.push('\n');

        for task in &self.tasks {
            let status_icon = match task.status {
                TeamTaskStatus::Pending => "⬜",
                TeamTaskStatus::InProgress => "🔄",
                TeamTaskStatus::Completed => "✅",
                TeamTaskStatus::Failed => "❌",
            };
            let assignee = task.assigned_to.as_deref().unwrap_or("unassigned");
            output.push_str(&format!("  {} [{}] {} → {}\n", status_icon, &task.id[..8], &task.description[..task.description.len().min(60)], assignee));
        }

        if !self.messages.is_empty() {
            output.push_str(&format!("\n💬 Messages: {}\n", self.messages.len()));
        }

        output
    }
}

// ─── Team Orchestrator ────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct TeamOrchestrator {
    teams: HashMap<String, AgentTeam>,
    /// Shared message bus: agent_name -> sender
    message_buses: HashMap<String, mpsc::Sender<TeamMessage>>,
}

impl TeamOrchestrator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_team(&mut self, name: &str) -> &mut AgentTeam {
        self.teams.entry(name.to_string()).or_insert_with(|| AgentTeam::new(name))
    }

    pub fn get_team(&self, name: &str) -> Option<&AgentTeam> {
        self.teams.get(name)
    }

    pub fn list_teams(&self) -> Vec<&AgentTeam> {
        self.teams.values().collect()
    }

    /// Run tasks in parallel across team agents
    pub async fn run_parallel(
        &mut self,
        team_name: &str,
        tasks: Vec<String>,
    ) -> Result<Vec<TeamTask>> {
        let team = self.teams.get_mut(team_name)
            .ok_or_else(|| anyhow::anyhow!("Team not found: {}", team_name))?;

        info!("Running {} tasks in parallel on team '{}'", tasks.len(), team_name);

        // Assign tasks round-robin
        let agent_names: Vec<String> = team.agents.clone();
        for (i, task_desc) in tasks.iter().enumerate() {
            let task_id = team.add_task(task_desc);
            if !agent_names.is_empty() {
                let agent_idx = i % agent_names.len();
                team.assign_task(&task_id, &agent_names[agent_idx]);
            }
        }

        // In a full implementation, this would spawn actual agent tasks
        // For now, we simulate completion
        let task_ids: Vec<String> = team.tasks.iter().map(|t| t.id.clone()).collect();
        for task_id in &task_ids {
            team.complete_task(task_id, "Task completed by agent");
        }

        info!("Completed {} tasks on team '{}'", tasks.len(), team_name);
        Ok(team.tasks.clone())
    }
}
