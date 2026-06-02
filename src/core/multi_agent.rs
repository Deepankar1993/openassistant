// src/core/multi_agent.rs
//! Multi-agent routing (OpenClaw-style)
//! Run multiple isolated agents, each with workspace, persona, and sessions

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, debug};

/// An isolated agent definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub id: String,
    pub name: String,
    pub workspace_dir: String,
    pub model: String,
    pub persona_name: String,
    pub enabled: bool,
    pub channels: Vec<String>,  // Which channels route to this agent
}

/// Routes inbound messages to the right agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteBinding {
    pub channel: String,
    pub agent_id: String,
    pub priority: u32,  // Higher = checked first
}

/// Multi-agent router
#[derive(Debug, Default)]
pub struct MultiAgentRouter {
    agents: HashMap<String, AgentDef>,
    bindings: Vec<RouteBinding>,
}

impl MultiAgentRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an agent
    pub fn register_agent(&mut self, agent: AgentDef) {
        info!("Registered agent: {} ({})", agent.name, agent.id);
        self.agents.insert(agent.id.clone(), agent);
    }

    /// Add a route binding
    pub fn add_binding(&mut self, binding: RouteBinding) {
        self.bindings.push(binding);
        // Sort by priority (highest first)
        self.bindings.sort_by_key(|b| std::cmp::Reverse(b.priority));
    }

    /// Route a message from a channel to the right agent
    pub fn route(&self, channel: &str) -> Option<&AgentDef> {
        for binding in &self.bindings {
            if binding.channel == channel {
                if let Some(agent) = self.agents.get(&binding.agent_id) {
                    if agent.enabled {
                        debug!("Routing channel '{}' to agent '{}'", channel, agent.name);
                        return Some(agent);
                    }
                }
            }
        }
        None
    }

    /// Get an agent by ID
    pub fn get_agent(&self, id: &str) -> Option<&AgentDef> {
        self.agents.get(id)
    }

    /// List all agents
    pub fn list_agents(&self) -> Vec<&AgentDef> {
        self.agents.values().collect()
    }

    /// Create a default single-agent setup
    pub fn default_single_agent(model: &str, workspace: &str) -> Self {
        let mut router = Self::new();
        router.register_agent(AgentDef {
            id: "main".to_string(),
            name: "openAssistant".to_string(),
            workspace_dir: workspace.to_string(),
            model: model.to_string(),
            persona_name: "default".to_string(),
            enabled: true,
            channels: vec!["cli".into(), "discord".into(), "telegram".into()],
        });
        router.add_binding(RouteBinding {
            channel: "cli".to_string(),
            agent_id: "main".to_string(),
            priority: 100,
        });
        router
    }
}
