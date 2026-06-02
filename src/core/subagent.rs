// src/core/subagent.rs
//! Sub-agent / delegate architecture (Hermes-style)
//! Spawn isolated parallel workers with their own context, tools, and workspace

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::oneshot;
use tracing::info;
use uuid::Uuid;

/// A sub-agent task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentTask {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub context: String,
    pub tools_allowed: Vec<String>,
    pub max_steps: usize,
    pub timeout_seconds: u64,
}

/// Result from a sub-agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub task_id: String,
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub steps_taken: usize,
    pub duration_ms: u64,
}

/// A spawned sub-agent handle
#[derive(Debug)]
pub struct SubAgentHandle {
    pub task: SubAgentTask,
    pub result_rx: oneshot::Receiver<SubAgentResult>,
}

/// Sub-agent orchestrator
#[derive(Debug, Default)]
pub struct SubAgentOrchestrator {
    running: HashMap<String, SubAgentHandle>,
    max_concurrent: usize,
}

impl SubAgentOrchestrator {
    pub fn new() -> Self {
        Self {
            running: HashMap::new(),
            max_concurrent: 3,
        }
    }

    /// Spawn a sub-agent to work on a task
    pub async fn spawn(
        &mut self,
        task: SubAgentTask,
        agent_model: &str,
        workspace_dir: &str,
    ) -> Result<String> {
        if self.running.len() >= self.max_concurrent {
            return Err(anyhow::anyhow!(
                "Max concurrent sub-agents ({}) reached. Wait for one to finish.",
                self.max_concurrent
            ));
        }

        let task_id = task.id.clone();
        let task_clone = task.clone();
        let model = agent_model.to_string();
        let workspace = workspace_dir.to_string();

        info!("Spawning sub-agent: {} — {}", task.name, &task.goal[..task.goal.len().min(80)]);

        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let result = execute_subagent(&task_clone, &model, &workspace).await;
            let duration = start.elapsed().as_millis() as u64;

            let (output, error) = match result {
                Ok(o) => (o, None),
                Err(e) => (format!("Error: {}", e), Some(e.to_string())),
            };

            let final_result = SubAgentResult {
                task_id: task_clone.id.clone(),
                success: error.is_none(),
                output,
                error,
                steps_taken: 0,
                duration_ms: duration,
            };

            let _ = tx.send(final_result);
        });

        self.running.insert(
            task_id.clone(),
            SubAgentHandle {
                task,
                result_rx: rx,
            },
        );

        Ok(task_id)
    }

    /// Check if a sub-agent is done and get its result
    pub async fn poll_result(&mut self, task_id: &str) -> Option<SubAgentResult> {
        if let Some(handle) = self.running.get_mut(task_id) {
            if let Ok(result) = handle.result_rx.try_recv() {
                self.running.remove(task_id);
                return Some(result);
            }
        }
        None
    }

    /// List running sub-agents
    pub fn list_running(&self) -> Vec<&SubAgentTask> {
        self.running.values().map(|h| &h.task).collect()
    }

    /// Create a sub-agent task
    pub fn create_task(name: &str, goal: &str) -> SubAgentTask {
        SubAgentTask {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            goal: goal.to_string(),
            context: String::new(),
            tools_allowed: vec!["browser".into(), "shell".into(), "file".into(), "web_search".into()],
            max_steps: 20,
            timeout_seconds: 300,
        }
    }
}

async fn execute_subagent(
    _task: &SubAgentTask,
    _model: &str,
    _workspace: &str,
) -> Result<String> {
    // In a full implementation, this would call the LLM in a loop
    Ok("Sub-agent task completed.".to_string())
}
