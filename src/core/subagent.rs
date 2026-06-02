// src/core/subagent.rs
//! Sub-agent system — YAML frontmatter definitions, isolated context windows
//! Sub-agents are defined in .claude/agents/*.md with YAML frontmatter
//! Each runs with its own context, tool set, and optional model override.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, debug, warn};
use tokio::sync::oneshot;
use uuid::Uuid;

// ─── Sub-Agent Definition (from .md frontmatter) ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentDef {
    pub name: String,
    pub description: String,
    /// Tools this sub-agent can use (None = all tools)
    pub tools: Option<Vec<String>>,
    /// Model override (None = use default)
    pub model: Option<String>,
    /// Custom system prompt
    pub system_prompt: Option<String>,
    /// Max iterations for the sub-agent loop
    pub max_iterations: Option<usize>,
    /// Timeout in seconds
    pub timeout_seconds: Option<u64>,
    /// Source file path
    pub file_path: Option<PathBuf>,
}

// ─── Raw frontmatter ───────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct SubAgentFrontmatter {
    name: Option<String>,
    description: Option<String>,
    tools: Option<Vec<String>>,
    model: Option<String>,
    #[serde(rename = "system-prompt")]
    system_prompt: Option<String>,
    #[serde(rename = "max-iterations")]
    max_iterations: Option<usize>,
    #[serde(rename = "timeout-seconds")]
    timeout_seconds: Option<u64>,
}

// ─── Sub-Agent Task ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentTask {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub context: String,
    pub tools_allowed: Vec<String>,
    pub max_steps: usize,
    pub timeout_seconds: u64,
    pub model: Option<String>,
}

// ─── Sub-Agent Result ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub task_id: String,
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub steps_taken: usize,
    pub duration_ms: u64,
}

// ─── Sub-Agent Handle ─────────────────────────────────────────────────

#[derive(Debug)]
pub struct SubAgentHandle {
    pub task: SubAgentTask,
    pub result_rx: oneshot::Receiver<SubAgentResult>,
}

// ─── Sub-Agent Orchestrator ───────────────────────────────────────────

#[derive(Debug, Default)]
pub struct SubAgentOrchestrator {
    running: HashMap<String, SubAgentHandle>,
    max_concurrent: usize,
    /// Loaded sub-agent definitions from .claude/agents/
    definitions: HashMap<String, SubAgentDef>,
}

impl SubAgentOrchestrator {
    pub fn new() -> Self {
        Self {
            running: HashMap::new(),
            max_concurrent: 3,
            definitions: HashMap::new(),
        }
    }

    /// Load sub-agent definitions from .claude/agents/ directory
    pub fn load_definitions(&mut self, agents_dir: &str) -> Result<usize> {
        let path = PathBuf::from(agents_dir);
        if !path.exists() {
            debug!("Agents directory does not exist: {}", agents_dir);
            return Ok(0);
        }

        let mut count = 0;
        for entry in walkdir::WalkDir::new(&path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|e| e.to_str()) == Some("md"))
        {
            match Self::parse_agent_file(&entry.path().to_path_buf()) {
                Ok(def) => {
                    info!("Loaded sub-agent definition: {}", def.name);
                    self.definitions.insert(def.name.clone(), def);
                    count += 1;
                }
                Err(e) => {
                    warn!("Failed to parse agent file {:?}: {}", entry.path(), e);
                }
            }
        }

        info!("Loaded {} sub-agent definitions from {}", count, agents_dir);
        Ok(count)
    }

    /// Parse a sub-agent .md file with YAML frontmatter
    fn parse_agent_file(path: &PathBuf) -> Result<SubAgentDef> {
        let content = std::fs::read_to_string(path)?;
        let (frontmatter, body) = parse_subagent_frontmatter(&content)?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let fm = frontmatter.unwrap_or_default();

        Ok(SubAgentDef {
            name: fm.name.unwrap_or_else(|| name.clone()),
            description: fm.description.unwrap_or_else(|| format!("Sub-agent: {}", name)),
            tools: fm.tools,
            model: fm.model,
            system_prompt: fm.system_prompt.or_else(|| {
                if body.trim().is_empty() {
                    None
                } else {
                    Some(body.trim().to_string())
                }
            }),
            max_iterations: fm.max_iterations,
            timeout_seconds: fm.timeout_seconds,
            file_path: Some(path.clone()),
        })
    }

    /// Spawn a sub-agent task
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
        let model = task.model.clone().unwrap_or_else(|| agent_model.to_string());
        let workspace = workspace_dir.to_string();

        info!(
            "Spawning sub-agent: {} — {}",
            task.name,
            &task.goal[..task.goal.len().min(80)]
        );

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

    pub fn list_running(&self) -> Vec<&SubAgentTask> {
        self.running.values().map(|h| &h.task).collect()
    }

    pub fn get_definition(&self, name: &str) -> Option<&SubAgentDef> {
        self.definitions.get(name)
    }

    pub fn list_definitions(&self) -> Vec<&SubAgentDef> {
        self.definitions.values().collect()
    }

    pub fn create_task(name: &str, goal: &str) -> SubAgentTask {
        SubAgentTask {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            goal: goal.to_string(),
            context: String::new(),
            tools_allowed: vec![
                "read".into(),
                "glob".into(),
                "grep".into(),
                "bash".into(),
            ],
            max_steps: 20,
            timeout_seconds: 300,
            model: None,
        }
    }
}

/// Execute a sub-agent task (simplified — in production this would run the full agent loop)
async fn execute_subagent(
    task: &SubAgentTask,
    _model: &str,
    _workspace: &str,
) -> Result<String> {
    // In a full implementation, this would:
    // 1. Create an isolated agent with the sub-agent's system prompt
    // 2. Run the agent loop with the allowed tools
    // 3. Return the final result
    Ok(format!(
        "Sub-agent '{}' completed task: {}\nTools used: {}\nSteps: {}/{}",
        task.name,
        &task.goal[..task.goal.len().min(100)],
        task.tools_allowed.join(", "),
        0,
        task.max_steps
    ))
}

// ─── YAML Frontmatter Parser ──────────────────────────────────────────

pub fn parse_subagent_frontmatter(content: &str) -> Result<(Option<SubAgentFrontmatter>, String)> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return Ok((None, content.to_string()));
    }

    let rest = &trimmed[3..];
    if let Some(end_pos) = rest.find("\n---") {
        let yaml_str = &rest[..end_pos];
        let body = &rest[end_pos + 4..];

        match serde_yaml::from_str::<SubAgentFrontmatter>(yaml_str) {
            Ok(fm) => Ok((Some(fm), body.trim_start().to_string())),
            Err(e) => {
                debug!("Failed to parse sub-agent frontmatter: {}", e);
                Ok((None, content.to_string()))
            }
        }
    } else {
        Ok((None, content.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_frontmatter() {
        let content = "---\
name: code-reviewer
description: Reviews code for quality and security
tools: [Read, Grep, Glob]
model: opus
max-iterations: 30
---
You are a code reviewer. Focus on security and correctness.";

        let (fm, body) = parse_subagent_frontmatter(content).unwrap();
        let fm = fm.unwrap();
        assert_eq!(fm.name.unwrap(), "code-reviewer");
        assert_eq!(fm.tools.unwrap(), vec!["Read", "Grep", "Glob"]);
        assert_eq!(fm.model.unwrap(), "opus");
        assert_eq!(fm.max_iterations.unwrap(), 30);
        assert!(body.contains("You are a code reviewer"));
    }
}
