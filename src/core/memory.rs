// src/core/memory.rs
//! Unified memory system combining:
//! - OpenClaw: MEMORY.md (long-term) + memory/YYYY-MM-DD.md (daily notes) + DREAMS.md
//! - Hermes: FTS5 session search with LLM summarization + Honcho user modeling
//! - OpenHumans: Data source tracking + project context

use anyhow::Result;
use std::path::PathBuf;
use tracing::{info, debug};

/// Memory workspace — file-based long-term + daily memory (OpenClaw-style)
pub struct MemoryWorkspace {
    pub base_dir: PathBuf,
}

impl MemoryWorkspace {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn from_data_dir(data_dir: &str) -> Self {
        Self::new(PathBuf::from(data_dir))
    }

    /// Get the MEMORY.md path (long-term curated memory)
    fn memory_file(&self) -> PathBuf {
        self.base_dir.join("MEMORY.md")
    }

    /// Get today's daily note path
    fn today_file(&self) -> PathBuf {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.base_dir.join("memory").join(format!("{}.md", today))
    }

    /// Get yesterday's daily note path
    fn yesterday_file(&self) -> PathBuf {
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        self.base_dir.join("memory").join(format!("{}.md", yesterday))
    }

    fn dreams_file(&self) -> PathBuf {
        self.base_dir.join("DREAMS.md")
    }

    /// Read long-term memory (MEMORY.md)
    pub fn read_long_term(&self) -> String {
        match std::fs::read_to_string(self.memory_file()) {
            Ok(content) => content,
            Err(_) => String::new(),
        }
    }

    /// Write to long-term memory (append mode)
    pub fn append_long_term(&self, content: &str) -> Result<()> {
        let path = self.memory_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let new_content = if existing.is_empty() {
            format!("# Long-Term Memory\n\n{}\n", content)
        } else {
            format!("{}\n{}\n", existing.trim(), content)
        };
        std::fs::write(&path, new_content)?;
        info!("Updated MEMORY.md");
        Ok(())
    }

    /// Overwrite long-term memory entirely (user-editable)
    pub fn write_long_term(&self, content: &str) -> Result<()> {
        let path = self.memory_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        info!("Overwrote MEMORY.md");
        Ok(())
    }

    /// Read today's daily notes
    pub fn read_today(&self) -> String {
        match std::fs::read_to_string(self.today_file()) {
            Ok(content) => content,
            Err(_) => String::new(),
        }
    }

    /// Read yesterday's daily notes
    pub fn read_yesterday(&self) -> String {
        match std::fs::read_to_string(self.yesterday_file()) {
            Ok(content) => content,
            Err(_) => String::new(),
        }
    }

    /// Append to today's daily notes
    pub fn append_daily(&self, content: &str) -> Result<()> {
        let path = self.today_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let now = chrono::Local::now().format("%H:%M").to_string();
        let new_content = if existing.is_empty() {
            format!("# Daily Notes — {}\n\n- [{}] {}\n", 
                chrono::Local::now().format("%Y-%m-%d"), now, content)
        } else {
            format!("{}\n- [{}] {}\n", existing.trim(), now, content)
        };
        std::fs::write(&path, new_content)?;
        debug!("Appended to daily notes");
        Ok(())
    }

    /// Search memory files for a keyword (simple grep-style)
    pub fn search_files(&self, query: &str) -> Vec<(String, String)> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();

        // Search MEMORY.md
        let lt = self.read_long_term();
        for (i, line) in lt.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                results.push(("MEMORY.md".to_string(), format!("L{}: {}", i + 1, line.trim())));
            }
        }

        // Search today's notes
        let today = self.read_today();
        for (i, line) in today.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                results.push(("today.md".to_string(), format!("L{}: {}", i + 1, line.trim())));
            }
        }

        // Search yesterday's notes
        let yesterday = self.read_yesterday();
        for (i, line) in yesterday.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                results.push(("yesterday.md".to_string(), format!("L{}: {}", i + 1, line.trim())));
            }
        }

        results
    }

    /// Full context to inject into system prompt
    pub fn build_context(&self) -> String {
        let mut ctx = String::new();

        let lt = self.read_long_term();
        if !lt.is_empty() {
            ctx.push_str("## Long-Term Memory (MEMORY.md)\n");
            ctx.push_str(&lt);
            ctx.push_str("\n\n");
        }

        let today = self.read_today();
        if !today.is_empty() {
            ctx.push_str("## Today's Notes\n");
            ctx.push_str(&today);
            ctx.push_str("\n\n");
        }

        let yesterday = self.read_yesterday();
        if !yesterday.is_empty() {
            ctx.push_str("## Yesterday's Notes\n");
            ctx.push_str(&yesterday);
            ctx.push('\n');
        }

        ctx
    }

    /// Initialize workspace with default files if they don't exist
    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.base_dir)?;
        std::fs::create_dir_all(self.base_dir.join("memory"))?;

        if !self.memory_file().exists() {
            std::fs::write(self.memory_file(), "# Long-Term Memory\n\n")?;
        }

        Ok(())
    }
}
