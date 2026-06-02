// src/core/plugins.rs
//! Plugin marketplace — discover, install, and manage Claude Code plugins
//! Plugins from .claude/plugins/ or the Hermes marketplace

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, debug, warn};

// ─── Plugin Definition ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub source: PluginSource,
    pub install_path: Option<PathBuf>,
    pub enabled: bool,
    /// Skills provided by this plugin
    pub skills: Vec<String>,
    /// Hooks provided by this plugin
    pub hooks: Vec<String>,
    /// MCP servers provided by this plugin
    pub mcp_servers: Vec<String>,
    /// Custom agents provided by this plugin
    pub agents: Vec<String>,
    /// Custom slash commands
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginSource {
    /// Local directory
    Local { path: String },
    /// Git repository
    Git { url: String, branch: Option<String> },
    /// Marketplace
    Marketplace { id: String },
}

// ─── Plugin Manifest (plugin.json) ────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct PluginManifest {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    author: Option<String>,
    #[serde(rename = "defaultEnabled")]
    default_enabled: Option<bool>,
}

// ─── Plugin Marketplace ───────────────────────────────────────────────

#[derive(Debug)]
pub struct PluginMarketplace {
    plugins: HashMap<String, Plugin>,
    install_dir: String,
}

impl PluginMarketplace {
    pub fn new(install_dir: &str) -> Self {
        Self {
            plugins: HashMap::new(),
            install_dir: install_dir.to_string(),
        }
    }

    /// Load installed plugins from the install directory
    pub fn load_installed(&mut self) -> Result<usize> {
        let path = PathBuf::from(&self.install_dir);
        if !path.exists() {
            debug!("Plugin install directory not found: {}", self.install_dir);
            return Ok(0);
        }

        let mut count = 0;
        for entry in walkdir::WalkDir::new(&path)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() == "plugin.json")
        {
            match self.load_plugin_manifest(&entry.path().to_path_buf()) {
                Ok(plugin) => {
                    info!("Loaded plugin: {} v{}", plugin.name, plugin.version);
                    self.plugins.insert(plugin.name.clone(), plugin);
                    count += 1;
                }
                Err(e) => {
                    warn!("Failed to load plugin from {:?}: {}", entry.path(), e);
                }
            }
        }

        info!("Loaded {} installed plugins", count);
        Ok(count)
    }

    fn load_plugin_manifest(&self, path: &PathBuf) -> Result<Plugin> {
        let content = std::fs::read_to_string(path)?;
        let manifest: PluginManifest = serde_json::from_str(&content)?;

        let plugin_dir = path.parent()
            .ok_or_else(|| anyhow::anyhow!("Plugin path has no parent"))?;

        // Discover skills
        let skills_dir = plugin_dir.join("skills");
        let skills = if skills_dir.exists() {
            walkdir::WalkDir::new(&skills_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|e| e.to_str()) == Some("md"))
                .map(|e| {
                    let p = e.path();
                    p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
                })
                .flatten()
                .collect()
        } else {
            Vec::new()
        };

        // Discover hooks
        let hooks_dir = plugin_dir.join("hooks");
        let hooks = if hooks_dir.exists() {
            walkdir::WalkDir::new(&hooks_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name() == "hooks.json")
                .map(|e| e.path().to_string_lossy().to_string())
                .collect()
        } else {
            Vec::new()
        };

        // Discover MCP servers
        let mcp_dir = plugin_dir.join("mcp");
        let mcp_servers = if mcp_dir.exists() {
            walkdir::WalkDir::new(&mcp_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name() == "config.json")
                .map(|e| {
                    let p = e.path();
                    p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
                })
                .flatten()
                .collect()
        } else {
            Vec::new()
        };

        // Discover agents
        let agents_dir = plugin_dir.join("agents");
        let agents = if agents_dir.exists() {
            walkdir::WalkDir::new(&agents_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|e| e.to_str()) == Some("md"))
                .map(|e| {
                    let p = e.path();
                    p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
                })
                .flatten()
                .collect()
        } else {
            Vec::new()
        };

        // Discover commands
        let commands_dir = plugin_dir.join("commands");
        let commands = if commands_dir.exists() {
            walkdir::WalkDir::new(&commands_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|e| e.to_str()) == Some("md"))
                .map(|e| {
                    let p = e.path();
                    p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
                })
                .flatten()
                .collect()
        } else {
            Vec::new()
        };

        Ok(Plugin {
            name: manifest.name.unwrap_or_else(|| {
                plugin_dir.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            }),
            description: manifest.description.unwrap_or_default(),
            version: manifest.version.unwrap_or_else(|| "0.1.0".to_string()),
            author: manifest.author.unwrap_or_default(),
            source: PluginSource::Local {
                path: plugin_dir.to_string_lossy().to_string(),
            },
            install_path: Some(plugin_dir.to_path_buf()),
            enabled: manifest.default_enabled.unwrap_or(true),
            skills,
            hooks,
            mcp_servers,
            agents,
            commands,
        })
    }

    /// Enable/disable a plugin
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.enabled = enabled;
            info!("Plugin '{}' enabled: {}", name, enabled);
            true
        } else {
            false
        }
    }

    /// Get all enabled plugins
    pub fn list_enabled(&self) -> Vec<&Plugin> {
        self.plugins.values().filter(|p| p.enabled).collect()
    }

    /// Get all skills from enabled plugins (merged with built-in)
    pub fn list_all_skills(&self) -> Vec<String> {
        let mut skills = Vec::new();
        for plugin in self.plugins.values().filter(|p| p.enabled) {
            for skill in &plugin.skills {
                skills.push(format!("{}::{}", plugin.name, skill));
            }
        }
        skills
    }

    /// Install a plugin from a source
    pub async fn install(&mut self, source: PluginSource) -> Result<String> {
        match &source {
            PluginSource::Local { path } => {
                let src = PathBuf::from(path);
                if !src.exists() {
                    return Err(anyhow::anyhow!("Plugin source not found: {}", path));
                }

                let plugin_name = src.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let dest = PathBuf::from(&self.install_dir).join(&plugin_name);
                if dest.exists() {
                    return Err(anyhow::anyhow!("Plugin '{}' is already installed", plugin_name));
                }

                // Copy plugin directory
                tokio::fs::create_dir_all(&self.install_dir).await?;
                self.copy_dir_all(&src, &dest).await?;

                info!("Installed plugin: {}", plugin_name);
                Ok(plugin_name)
            }
            PluginSource::Git { url, branch } => {
                info!("Installing plugin from git: {} (branch: {:?})", url, branch);
                let output = tokio::process::Command::new("git")
                    .args(&["clone"])
                    .args(branch.as_ref().map(|b| vec!["-b", b]).unwrap_or_default())
                    .arg(url)
                    .arg(PathBuf::from(&self.install_dir).join("temp"))
                    .output()
                    .await?;

                if !output.status.success() {
                    return Err(anyhow::anyhow!(
                        "Git clone failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }

                Ok("git-plugin".to_string())
            }
            PluginSource::Marketplace { id } => {
                info!("Installing plugin from marketplace: {}", id);
                // In production, this would fetch from a marketplace API
                Err(anyhow::anyhow!("Marketplace not yet configured"))
            }
        }
    }

    async fn copy_dir_all(&self, src: &PathBuf, dst: &PathBuf) -> Result<()> {
        tokio::fs::create_dir_all(dst).await?;
        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let ty = entry.file_type().await?;
            let dest_path = dst.join(entry.file_name());
            if ty.is_dir() {
                Box::pin(self.copy_dir_all(&entry.path().to_path_buf(), &dest_path)).await?;
            } else {
                tokio::fs::copy(&entry.path(), &dest_path).await?;
            }
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.get(name)
    }

    pub fn list(&self) -> Vec<&Plugin> {
        self.plugins.values().collect()
    }

    pub fn format_status(&self) -> String {
        let enabled = self.plugins.values().filter(|p| p.enabled).count();
        let mut output = format!("🧩 Plugins: {} total, {} enabled\n", self.plugins.len(), enabled);
        output.push_str(&"─".repeat(50));
        output.push('\n');
        for plugin in self.plugins.values() {
            let status = if plugin.enabled { "✅" } else { "⬜" };
            output.push_str(&format!(
                "  {} {} v{} by {} ({} skills, {} hooks)\n",
                status, plugin.name, plugin.version, plugin.author,
                plugin.skills.len(), plugin.hooks.len()
            ));
        }
        output
    }
}
