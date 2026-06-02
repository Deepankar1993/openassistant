// src/skills/engine.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, debug};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub category: String,
    pub content: String, // Markdown content
    pub file_path: Option<PathBuf>,
    pub auto_created: bool,
}

#[derive(Debug, Default)]
pub struct SkillEngine {
    skills: Vec<Skill>,
}

impl SkillEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_builtin() -> Result<Self> {
        let mut engine = Self::new();
        engine.load_builtins();
        Ok(engine)
    }

    fn load_builtins(&mut self) {
        self.skills.push(Skill {
            name: "coding".to_string(),
            description: "Help with programming tasks, debugging, and code review".to_string(),
            category: "development".to_string(),
            content: include_str!("../../skills/coding.md").to_string(),
            file_path: None,
            auto_created: false,
        });
        self.skills.push(Skill {
            name: "research".to_string(),
            description: "Search, analyze, and synthesize information".to_string(),
            category: "research".to_string(),
            content: include_str!("../../skills/research.md").to_string(),
            file_path: None,
            auto_created: false,
        });
        self.skills.push(Skill {
            name: "writing".to_string(),
            description: "Draft, edit, and improve written content".to_string(),
            category: "creative".to_string(),
            content: include_str!("../../skills/writing.md").to_string(),
            file_path: None,
            auto_created: false,
        });
        info!("Loaded {} built-in skills", self.skills.len());
    }

    pub fn load_from_dir(&mut self, dir: &str) -> Result<usize> {
        let path = PathBuf::from(dir);
        if !path.exists() {
            debug!("Skills directory does not exist: {}", dir);
            return Ok(0);
        }

        let mut count = 0;
        for entry in walkdir::WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("md") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    let name = entry.path()
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    self.skills.push(Skill {
                        name: name.clone(),
                        description: format!("Skill: {}", name),
                        category: "custom".to_string(),
                        content,
                        file_path: Some(entry.path().to_path_buf()),
                        auto_created: false,
                    });
                    count += 1;
                }
            }
        }
        info!("Loaded {} skills from {}", count, dir);
        Ok(count)
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    pub fn count(&self) -> usize {
        self.skills.len()
    }

    pub fn matching(&self, query: &str) -> Vec<&Skill> {
        let query_lower = query.to_lowercase();
        self.skills
            .iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&query_lower)
                    || s.description.to_lowercase().contains(&query_lower)
                    || s.category.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Auto-create a skill from a completed task (Hermes-style learning loop)
    pub fn auto_create(&mut self, task_description: &str, approach: &str) -> Result<()> {
        let name = format!("auto-{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
        let content = format!(
            "# {}\n\nAuto-created from task.\n\n## Task\n{}\n\n## Approach\n{}\n",
            name, task_description, approach
        );

        self.skills.push(Skill {
            name: name.clone(),
            description: format!("Auto-created skill from: {}", task_description),
            category: "auto".to_string(),
            content,
            file_path: None,
            auto_created: true,
        });

        info!("Auto-created skill: {}", name);
        Ok(())
    }
}
