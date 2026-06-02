// src/skills/engine.rs
//! Skills engine — YAML frontmatter parsing, auto-loading from .claude/skills/
//! Claude Code-compatible skill definitions with allowed/disallowed tools.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, debug, warn};

// ─── Skill Definition with YAML Frontmatter ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub category: String,
    pub content: String,
    pub file_path: Option<PathBuf>,
    pub auto_created: bool,
    // YAML frontmatter fields
    pub version: Option<String>,
    pub author: Option<String>,
    pub platforms: Option<Vec<String>>,
    // Tools this skill is allowed to use (empty = all allowed)
    pub allowed_tools: Option<Vec<String>>,
    // Tools this skill explicitly disallows
    pub disallowed_tools: Option<Vec<String>>,
    // Whether skill is enabled by default
    pub default_enabled: Option<bool>,
    // Metadata
    pub metadata: Option<HashMap<String, String>>,
}

impl Default for Skill {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            category: String::from("general"),
            content: String::new(),
            file_path: None,
            auto_created: false,
            version: None,
            author: None,
            platforms: None,
            allowed_tools: None,
            disallowed_tools: None,
            default_enabled: Some(true),
            metadata: None,
        }
    }
}

// ─── Raw frontmatter parsed from YAML ─────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    author: Option<String>,
    platforms: Option<Vec<String>>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(rename = "disallowed-tools")]
    disallowed_tools: Option<Vec<String>>,
    #[serde(rename = "default-enabled")]
    default_enabled: Option<bool>,
    metadata: Option<HashMap<String, String>>,
}

// ─── Skill Engine ─────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct SkillEngine {
    skills: Vec<Skill>,
    /// Currently active skill (if any) — restricts tool access
    active_skill: Option<String>,
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
        let builtins = vec![
            ("coding", "development", "Help with programming tasks, debugging, and code review",
             include_str!("../../skills/coding.md")),
            ("research", "research", "Search, analyze, and synthesize information",
             include_str!("../../skills/research.md")),
            ("writing", "creative", "Draft, edit, and improve written content",
             include_str!("../../skills/writing.md")),
        ];

        for (name, category, desc, content) in builtins {
            self.skills.push(Skill {
                name: name.to_string(),
                description: desc.to_string(),
                category: category.to_string(),
                content: content.to_string(),
                file_path: None,
                auto_created: false,
                version: Some("1.0.0".to_string()),
                author: Some("openAssistant".to_string()),
                platforms: Some(vec!["linux".into(), "macos".into(), "windows".into()]),
                allowed_tools: None,
                disallowed_tools: None,
                default_enabled: Some(true),
                metadata: None,
            });
        }
        info!("Loaded {} built-in skills", self.skills.len());
    }

    /// Load skills from a directory (like .claude/skills/)
    pub fn load_from_dir(&mut self, dir: &str) -> Result<usize> {
        let path = PathBuf::from(dir);
        if !path.exists() {
            debug!("Skills directory does not exist: {}", dir);
            return Ok(0);
        }

        let mut count = 0;
        for entry in walkdir::WalkDir::new(&path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|e| e.to_str()) == Some("md"))
        {
            match self.load_skill_from_file(&entry.path().to_path_buf()) {
                Ok(skill) => {
                    debug!("Loaded skill: {} from {:?}", skill.name, entry.path());
                    self.skills.push(skill);
                    count += 1;
                }
                Err(e) => {
                    warn!("Failed to load skill from {:?}: {}", entry.path(), e);
                }
            }
        }

        info!("Loaded {} skills from {}", count, dir);
        Ok(count)
    }

    /// Load a single skill from a .md file with YAML frontmatter
    pub fn load_skill_from_file(&self, path: &PathBuf) -> Result<Skill> {
        let content = std::fs::read_to_string(path)?;
        let (frontmatter, body) = parse_skill_frontmatter(&content)?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let fm = frontmatter.unwrap_or_default();

        Ok(Skill {
            name: fm.name.clone().unwrap_or_else(|| name.clone()),
            description: fm.description.unwrap_or_else(|| format!("Skill: {}", name)),
            category: "user".to_string(),
            content: body,
            file_path: Some(path.to_path_buf()),
            auto_created: false,
            version: fm.version,
            author: fm.author,
            platforms: fm.platforms,
            allowed_tools: fm.allowed_tools,
            disallowed_tools: fm.disallowed_tools,
            default_enabled: fm.default_enabled,
            metadata: fm.metadata,
        })
    }

    /// Activate a skill by name — restricts tool access
    pub fn activate_skill(&mut self, name: &str) -> bool {
        if self.skills.iter().any(|s| s.name == name) {
            self.active_skill = Some(name.to_string());
            info!("Activated skill: {}", name);
            true
        } else {
            warn!("Skill not found: {}", name);
            false
        }
    }

    pub fn deactivate_skill(&mut self) {
        self.active_skill = None;
    }

    /// Get list of tools disallowed by the active skill
    pub fn get_disallowed_tools(&self) -> Vec<String> {
        if let Some(ref skill_name) = self.active_skill {
            if let Some(skill) = self.skills.iter().find(|s| &s.name == skill_name) {
                return skill.disallowed_tools.clone().unwrap_or_default();
            }
        }
        Vec::new()
    }

    /// Check if a tool is allowed given the active skill
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if let Some(ref skill_name) = self.active_skill {
            if let Some(skill) = self.skills.iter().find(|s| &s.name == skill_name) {
                // If disallowed_tools contains this tool, deny it
                if let Some(ref disallowed) = skill.disallowed_tools {
                    if disallowed.iter().any(|d| d == tool_name) {
                        return false;
                    }
                }
                // If allowed_tools is specified and non-empty, tool must be in it
                if let Some(ref allowed) = skill.allowed_tools {
                    if !allowed.is_empty() {
                        return allowed.iter().any(|a| a == tool_name);
                    }
                }
            }
        }
        true
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    pub fn list_active(&self) -> Option<&Skill> {
        self.active_skill.as_ref().and_then(|name| self.get(name))
    }

    pub fn count(&self) -> usize {
        self.skills.len()
    }
}

// ─── YAML Frontmatter Parser ──────────────────────────────────────────

/// Parse a skill markdown file with YAML frontmatter delimited by ---
/// Returns (Option<SkillFrontmatter>, body_content)
pub fn parse_skill_frontmatter(content: &str) -> Result<(Option<SkillFrontmatter>, String)> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return Ok((None, content.to_string()));
    }

    // Find the closing ---
    let rest = &trimmed[3..];
    if let Some(end_pos) = rest.find("\n---") {
        let yaml_str = &rest[..end_pos];
        let body = &rest[end_pos + 4..]; // Skip past \n---

        match serde_yaml::from_str::<SkillFrontmatter>(yaml_str) {
            Ok(fm) => Ok((Some(fm.trimmed_string_vals()), body.trim_start().to_string())),
            Err(e) => {
                debug!("Failed to parse YAML frontmatter: {}", e);
                Ok((None, content.to_string()))
            }
        }
    } else {
        Ok((None, content.to_string()))
    }
}

impl SkillFrontmatter {
    fn trimmed_string_vals(mut self) -> Self {
        self.name = self.name.map(|s| s.trim().to_string());
        self.description = self.description.map(|s| s.trim().to_string());
        self.author = self.author.map(|s| s.trim().to_string());
        self
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_basic() {
        let content = "---\
name: test-skill
description: A test skill
version: 1.0.0
allowed-tools: [Read, Edit]
disallowed-tools: [Bash]
---
# Test Skill Content
This is the body.";

        let (fm, body) = parse_skill_frontmatter(content).unwrap();
        let fm = fm.unwrap();
        assert_eq!(fm.name.unwrap(), "test-skill");
        assert_eq!(fm.description.unwrap(), "A test skill");
        assert_eq!(fm.version.unwrap(), "1.0.0");
        assert_eq!(fm.allowed_tools.unwrap(), vec!["Read", "Edit"]);
        assert_eq!(fm.disallowed_tools.unwrap(), vec!["Bash"]);
        assert!(body.contains("# Test Skill Content"));
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let content = "# Just a regular markdown file\nNo frontmatter here.";
        let (fm, body) = parse_skill_frontmatter(content).unwrap();
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_skill_tool_restrictions() {
        let mut engine = SkillEngine::new();

        let skill_content = "---\
name: reviewer
description: Code review skill
disallowed-tools: [Write, Bash]
---";
        let (fm, body) = parse_skill_frontmatter(skill_content).unwrap();
        let fm = fm.unwrap();

        engine.skills.push(Skill {
            name: fm.name.unwrap(),
            description: fm.description.unwrap(),
            category: "test".into(),
            content: body,
            file_path: None,
            auto_created: false,
            version: None,
            author: None,
            platforms: None,
            allowed_tools: None,
            disallowed_tools: fm.disallowed_tools,
            default_enabled: None,
            metadata: None,
        });

        // Without active skill, all tools allowed
        assert!(engine.is_tool_allowed("Write"));
        assert!(engine.is_tool_allowed("Read"));

        // Activate the skill
        engine.activate_skill("reviewer");

        // Now Write and Bash should be disallowed
        assert!(!engine.is_tool_allowed("Write"));
        assert!(!engine.is_tool_allowed("Bash"));
        // Read should still be allowed
        assert!(engine.is_tool_allowed("Read"));
    }
}
