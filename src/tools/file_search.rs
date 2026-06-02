// src/tools/file_search.rs
//! File search tools — Claude Code-style Glob and Grep
//! Glob: find files by pattern using walkdir
//! Grip: search file contents using regex

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

// ─── Glob Tool ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobArgs {
    pub pattern: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobResult {
    pub files: Vec<String>,
    pub total_found: usize,
}

/// Find files by glob pattern — Claude Code Glob tool equivalent
pub async fn glob(args: &serde_json::Value) -> Result<GlobResult> {
    let parsed: GlobArgs = serde_json::from_value(args.clone())?;
    
    let search_dir = parsed.path.clone().unwrap_or_else(|| ".".to_string());
    let max_results = parsed.max_results.unwrap_or(100);
    
    info!("Glob: {} in {}", parsed.pattern, search_dir);

    let pattern = glob::Pattern::new(&parsed.pattern)
        .map_err(|e| anyhow::anyhow!("Invalid glob pattern: {}", e))?;

    let mut files = Vec::new();
    let walk = walkdir::WalkDir::new(&search_dir)
        .follow_links(true)
        .into_iter();

    for entry in walk {
        if files.len() >= max_results {
            break;
        }
        if let Ok(entry) = entry {
            if entry.file_type().is_file() {
                let path_str = entry.path().to_string_lossy().to_string();
                // Match against both full path and file name
                let file_name = entry.file_name().to_string_lossy().to_string();
                if pattern.matches(&path_str) || pattern.matches(&file_name) {
                    files.push(path_str);
                }
            }
        }
    }

    let total = files.len();
    Ok(GlobResult { files, total_found: total })
}

// ─── Grep Tool ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepArgs {
    pub pattern: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub glob: Option<String>, // Filter by file pattern, e.g., "*.rs"
    #[serde(default)]
    pub max_results: Option<usize>,
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    #[serde(default)]
    pub context_lines: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepMatch {
    pub file: String,
    pub line_number: usize,
    pub line: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepResult {
    pub matches: Vec<GrepMatch>,
    pub total_matches: usize,
    pub files_searched: usize,
}

/// Search file contents — Claude Code Grep tool equivalent
pub async fn grep(args: &serde_json::Value) -> Result<GrepResult> {
    let parsed: GrepArgs = serde_json::from_value(args.clone())?;
    
    let search_dir = parsed.path.clone().unwrap_or_else(|| ".".to_string());
    let max_results = parsed.max_results.unwrap_or(50);
    let context = parsed.context_lines.unwrap_or(2);
    let case_sensitive = parsed.case_sensitive.unwrap_or(false);

    info!("Grep: {} in {}", parsed.pattern, search_dir);

    let regex_flags = if case_sensitive {
        ""
    } else {
        "(?i)"
    };
    let full_pattern = format!("{}{}", regex_flags, parsed.pattern);
    let re = regex::Regex::new(&full_pattern)
        .map_err(|e| anyhow::anyhow!("Invalid regex: {}", e))?;

    // Compile file glob filter if provided
    let file_filter = parsed.glob.as_ref().and_then(|g| glob::Pattern::new(g).ok());

    let mut matches = Vec::new();
    let mut files_searched = 0;
    let walk = walkdir::WalkDir::new(&search_dir)
        .follow_links(true)
        .into_iter();

    'outer: for entry in walk {
        if matches.len() >= max_results {
            break;
        }
        if let Ok(entry) = entry {
            if entry.file_type().is_file() {
                let path = entry.path();
                let path_str = path.to_string_lossy().to_string();

                // Apply file filter
                if let Some(ref filter) = file_filter {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if !filter.matches(&fname) && !filter.matches(&path_str) {
                        continue;
                    }
                }

                files_searched += 1;

                // Read file and search
                if let Ok(content) = std::fs::read_to_string(path) {
                    let lines: Vec<&str> = content.lines().collect();

                    for (idx, line) in lines.iter().enumerate() {
                        if re.is_match(line) {
                            let context_start = idx.saturating_sub(context);
                            let context_end = (idx + context + 1).min(lines.len());
                            
                            let before: Vec<String> = lines[context_start..idx]
                                .iter()
                                .map(|s| s.to_string())
                                .collect();
                            let after: Vec<String> = lines[(idx + 1)..context_end]
                                .iter()
                                .map(|s| s.to_string())
                                .collect();

                            matches.push(GrepMatch {
                                file: path_str.clone(),
                                line_number: idx + 1,
                                line: line.to_string(),
                                context_before: before,
                                context_after: after,
                            });

                            if matches.len() >= max_results {
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(GrepResult {
        total_matches: matches.len(),
        matches,
        files_searched,
    })
}

fn _push_str(buf: &mut String, s: &str) {
    buf.push_str(s);
    buf.push('\n');
}
