// src/core/workflows.rs
//! Dynamic workflow orchestration — parallel fan-out/fan-in for large tasks
//! Like Claude Code's dynamic workflows (10s-100s of parallel agents)

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::{info, debug};

// ─── Workflow Definition ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub description: String,
    pub agent_type: Option<String>,
    pub tools: Option<Vec<String>>,
    /// Steps this step depends on (must complete first)
    pub depends_on: Option<Vec<String>>,
    /// Whether this step can run in parallel with siblings
    #[serde(default)]
    pub parallel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
}

// ─── Workflow State ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct WorkflowRun {
    pub id: String,
    pub name: String,
    pub status: WorkflowStatus,
    pub steps: HashMap<String, StepResult>,
    pub total_steps: usize,
    pub completed_steps: usize,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_id: String,
    pub status: WorkflowStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

// ─── Workflow Engine ──────────────────────────────────────────────────

#[derive(Debug)]
pub struct WorkflowEngine {
    workflows: HashMap<String, WorkflowDef>,
    active_runs: HashMap<String, Arc<Mutex<WorkflowRun>>>,
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkflowEngine {
    pub fn new() -> Self {
        Self {
            workflows: HashMap::new(),
            active_runs: HashMap::new(),
        }
    }

    pub fn register_workflow(&mut self, def: WorkflowDef) {
        info!("Registered workflow: {} ({} steps)", def.name, def.steps.len());
        self.workflows.insert(def.name.clone(), def);
    }

    /// Execute a workflow with parallel fan-out/fan-in
    pub async fn execute(&mut self, workflow_name: &str) -> Result<String> {
        let workflow = self.workflows.get(workflow_name)
            .ok_or_else(|| anyhow::anyhow!("Workflow not found: {}", workflow_name))?
            .clone();

        let run_id = format!("wf_{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
        info!("Starting workflow run: {} ({})", run_id, workflow_name);

        let run = WorkflowRun {
            id: run_id.clone(),
            name: workflow_name.to_string(),
            status: WorkflowStatus::Running,
            steps: HashMap::new(),
            total_steps: workflow.steps.len(),
            completed_steps: 0,
            start_time: chrono::Utc::now(),
            end_time: None,
        };

        let run_arc = Arc::new(Mutex::new(run));
        self.active_runs.insert(run_id.clone(), run_arc.clone());

        // Execute steps respecting dependencies
        let mut completed = HashMap::new();
        let mut remaining: Vec<_> = workflow.steps.clone();

        while !remaining.is_empty() {
            // Find steps whose dependencies are all met
            let ready: Vec<_> = remaining
                .iter()
                .filter(|step| {
                    step.depends_on.as_ref().map_or(true, |deps| {
                        deps.iter().all(|d| completed.contains_key(d))
                    })
                })
                .cloned()
                .collect();

            if ready.is_empty() {
                return Err(anyhow::anyhow!("Workflow deadlock — no ready steps"));
            }

            // Execute ready steps in parallel
            let mut handles = Vec::new();
            for step in &ready {
                let step_clone = step.clone();
                let handle = tokio::spawn(async move {
                    let start = chrono::Utc::now();
                    debug!("Executing workflow step: {}", step_clone.id);

                    // Simulate step execution
                    // In production, this would spawn an actual agent with the step's config
                    let output = format!(
                        "Step '{}' completed: {}",
                        step_clone.id,
                        step_clone.description
                    );

                    StepResult {
                        step_id: step_clone.id.clone(),
                        status: WorkflowStatus::Completed,
                        output: Some(output),
                        error: None,
                        start_time: start,
                        end_time: Some(chrono::Utc::now()),
                    }
                });
                handles.push(handle);
            }

            // Wait for all parallel steps to complete
            for handle in handles {
                match handle.await {
                    Ok(result) => {
                        completed.insert(result.step_id.clone(), result);
                    }
                    Err(e) => {
                        tracing::warn!("Workflow step failed: {}", e);
                    }
                }
            }

            // Remove completed steps from remaining
            remaining.retain(|s| !completed.contains_key(&s.id));
        }

        // Mark run as completed
        let mut run = run_arc.lock().await;
        run.status = WorkflowStatus::Completed;
        run.completed_steps = run.total_steps;
        run.end_time = Some(chrono::Utc::now());
        run.steps = completed.clone();

        info!(
            "Workflow run {} completed: {}/{} steps",
            run_id,
            run.completed_steps,
            run.total_steps
        );

        Ok(format!(
            "Workflow '{}' completed: {}/{} steps in {:?}",
            workflow_name,
            run.completed_steps,
            run.total_steps,
            run.end_time.unwrap() - run.start_time
        ))
    }

    /// Fan-out: spawn N parallel agents for a list of tasks
    pub async fn fan_out(&self, tasks: Vec<String>) -> Vec<String> {
        let mut handles = Vec::new();
        for (i, task) in tasks.iter().enumerate() {
            let task = task.clone();
            let handle = tokio::spawn(async move {
                debug!("Fan-out agent {}: {}", i, &task[..task.len().min(50)]);
                format!("Agent {} completed: {}", i, &task[..task.len().min(100)])
            });
            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }

        info!("Fan-out completed: {} results", results.len());
        results
    }

    /// Fan-in: collect and combine results from multiple agents
    pub async fn fan_in(&self, results: Vec<String>) -> String {
        format!("Fan-in: combined {} agent results:\n{}", results.len(), results.join("\n"))
    }

    pub fn get_run(&self, run_id: &str) -> Option<Arc<Mutex<WorkflowRun>>> {
        self.active_runs.get(run_id).cloned()
    }

    pub fn list_runs(&self) -> Vec<&WorkflowRun> {
        // Can't return references from Arc<Mutex<>> without locking
        Vec::new()
    }

    pub fn list_workflows(&self) -> Vec<&WorkflowDef> {
        self.workflows.values().collect()
    }
}

// ─── Built-in Workflows ───────────────────────────────────────────────

pub fn built_in_workflows() -> Vec<WorkflowDef> {
    vec![
        WorkflowDef {
            name: "code-review".to_string(),
            description: "Multi-agent code review workflow".to_string(),
            steps: vec![
                WorkflowStep {
                    id: "explore".to_string(),
                    description: "Explore codebase and find changed files".to_string(),
                    agent_type: Some("Explore".to_string()),
                    tools: Some(vec!["glob".into(), "grep".into(), "read".into()]),
                    depends_on: None,
                    parallel: false,
                },
                WorkflowStep {
                    id: "review-security".to_string(),
                    description: "Security review of changes".to_string(),
                    agent_type: Some("General".to_string()),
                    tools: Some(vec!["read".into(), "grep".into()]),
                    depends_on: Some(vec!["explore".to_string()]),
                    parallel: true,
                },
                WorkflowStep {
                    id: "review-quality".to_string(),
                    description: "Code quality review".to_string(),
                    agent_type: Some("General".to_string()),
                    tools: Some(vec!["read".into(), "grep".into()]),
                    depends_on: Some(vec!["explore".to_string()]),
                    parallel: true,
                },
                WorkflowStep {
                    id: "summarize".to_string(),
                    description: "Summarize all review findings".to_string(),
                    agent_type: Some("General".to_string()),
                    tools: None,
                    depends_on: Some(vec!["review-security".into(), "review-quality".into()]),
                    parallel: false,
                },
            ],
        },
    ]
}
