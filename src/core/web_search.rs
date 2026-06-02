// src/core/web_search.rs
//! Multi-source web search (Hermes Tool Gateway-style)
//! Search across multiple engines: Brave, DuckDuckGo, Perplexity, Exa, Firecrawl, Tavily, etc.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, debug};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub default_engine: String,
    pub brave_api_key: String,
    pub perplexity_api_key: String,
    pub exa_api_key: String,
    pub firecrawl_api_key: String,
    pub tavily_api_key: String,
    pub max_results: usize,
    pub timeout_seconds: u64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_engine: "duckduckgo".to_string(),
            brave_api_key: String::new(),
            perplexity_api_key: String::new(),
            exa_api_key: String::new(),
            firecrawl_api_key: String::new(),
            tavily_api_key: String::new(),
            max_results: 10,
            timeout_seconds: 15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String,     // Which engine returned this
    pub relevance: f32,     // 0.0 to 1.0
}

#[derive(Debug, Clone)]
pub struct WebSearch {
    config: SearchConfig,
}

impl WebSearch {
    pub fn new(config: SearchConfig) -> Self {
        Self { config }
    }

    pub fn default() -> Self {
        Self { config: SearchConfig::default() }
    }

    /// Search using the default engine
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        self.search_with(&self.config.default_engine, query).await
    }

    /// Search with a specific engine
    pub async fn search_with(&self, engine: &str, query: &str) -> Result<Vec<SearchResult>> {
        info!("Searching '{}' with engine: {}", query, engine);

        match engine.to_lowercase().as_str() {
            "duckduckgo" | "ddg" => self.search_duckduckgo(query).await,
            "brave" => self.search_brave(query).await,
            "perplexity" => self.search_perplexity(query).await,
            "exa" => self.search_exa(query).await,
            "firecrawl" => self.search_firecrawl(query).await,
            "tavily" => self.search_tavily(query).await,
            "google" => self.search_google(query).await,
            _ => {
                debug!("Unknown engine '{}', falling back to duckduckgo", engine);
                self.search_duckduckgo(query).await
            }
        }
    }

    /// Search multiple engines and merge results (deduped by URL)
    pub async fn search_all(&self, query: &str) -> Result<Vec<SearchResult>> {
        let mut all_results = Vec::new();
        let engines = vec!["duckduckgo", "brave", "tavily"];

        for engine in engines {
            match self.search_with(engine, query).await {
                Ok(results) => {
                    for r in results {
                        if !all_results.iter().any(|existing: &SearchResult| existing.url == r.url) {
                            all_results.push(r);
                        }
                    }
                }
                Err(e) => {
                    debug!("Engine '{}' failed: {}", engine, e);
                }
            }
        }

        // Sort by relevance
        all_results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap());
        all_results.truncate(self.config.max_results);

        Ok(all_results)
    }

    // --- Engine implementations ---

    async fn search_duckduckgo(&self, query: &str) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            urlencoding::encode(query)
        );

        let client = reqwest::Client::new();
        let resp = client.get(&url)
            .header("User-Agent", "openAssistant/1.0")
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await.unwrap_or_default();
        let mut results = Vec::new();

        // Parse DuckDuckGo results
        if let Some(related) = json["RelatedTopics"].as_array() {
            for (i, topic) in related.iter().enumerate().take(self.config.max_results) {
                if let Some(text) = topic["Text"].as_str() {
                    let url = topic["FirstURL"].as_str().unwrap_or("").to_string();
                    results.push(SearchResult {
                        title: text.lines().next().unwrap_or(text).to_string(),
                        url,
                        snippet: text.to_string(),
                        source: "DuckDuckGo".to_string(),
                        relevance: 1.0 - (i as f32 * 0.1),
                    });
                }
            }
        }

        Ok(results)
    }

    async fn search_brave(&self, query: &str) -> Result<Vec<SearchResult>> {
        if self.config.brave_api_key.is_empty() {
            return self.search_duckduckgo(query).await;
        }

        let url = format!(
            "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
            urlencoding::encode(query),
            self.config.max_results
        );

        let client = reqwest::Client::new();
        let resp = client.get(&url)
            .header("Accept", "application/json")
            .header("Accept-Encoding", "gzip")
            .header("X-Subscription-Token", &self.config.brave_api_key)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await.unwrap_or_default();
        let mut results = Vec::new();

        if let Some(web) = json["web"]["results"].as_array() {
            for (i, r) in web.iter().enumerate() {
                results.push(SearchResult {
                    title: r["title"].as_str().unwrap_or("").to_string(),
                    url: r["url"].as_str().unwrap_or("").to_string(),
                    snippet: r["description"].as_str().unwrap_or("").to_string(),
                    source: "Brave".to_string(),
                    relevance: 1.0 - (i as f32 * 0.05),
                });
            }
        }

        Ok(results)
    }

    async fn search_perplexity(&self, query: &str) -> Result<Vec<SearchResult>> {
        if self.config.perplexity_api_key.is_empty() {
            return self.search_duckduckgo(query).await;
        }

        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "model": "sonar",
            "messages": [{"role": "user", "content": query}],
            "max_tokens": 1024,
        });

        let resp = client.post("https://api.perplexity.ai/chat/completions")
            .header("Authorization", format!("Bearer {}", self.config.perplexity_api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await.unwrap_or_default();
        let content = json["choices"][0]["message"]["content"]
            .as_str().unwrap_or("").to_string();

        Ok(vec![SearchResult {
            title: format!("Perplexity answer: {}", &query[..query.len().min(60)]),
            url: "https://perplexity.ai".to_string(),
            snippet: content,
            source: "Perplexity".to_string(),
            relevance: 1.0,
        }])
    }

    async fn search_exa(&self, query: &str) -> Result<Vec<SearchResult>> {
        if self.config.exa_api_key.is_empty() {
            return self.search_duckduckgo(query).await;
        }

        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "query": query,
            "numResults": self.config.max_results,
            "useAutoprompt": true,
        });

        let resp = client.post("https://api.exa.ai/search")
            .header("x-api-key", &self.config.exa_api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await.unwrap_or_default();
        let mut results = Vec::new();

        if let Some(items) = json["results"].as_array() {
            for (i, r) in items.iter().enumerate() {
                results.push(SearchResult {
                    title: r["title"].as_str().unwrap_or("").to_string(),
                    url: r["url"].as_str().unwrap_or("").to_string(),
                    snippet: r["text"].as_str().unwrap_or("").to_string(),
                    source: "Exa".to_string(),
                    relevance: 1.0 - (i as f32 * 0.05),
                });
            }
        }

        Ok(results)
    }

    async fn search_firecrawl(&self, query: &str) -> Result<Vec<SearchResult>> {
        if self.config.firecrawl_api_key.is_empty() {
            return self.search_duckduckgo(query).await;
        }

        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "query": query,
            "limit": self.config.max_results,
        });

        let resp = client.post("https://api.firecrawl.dev/v1/search")
            .header("Authorization", format!("Bearer {}", self.config.firecrawl_api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await.unwrap_or_default();
        let mut results = Vec::new();

        if let Some(data) = json["data"].as_array() {
            for (i, r) in data.iter().enumerate() {
                results.push(SearchResult {
                    title: r["title"].as_str().unwrap_or("").to_string(),
                    url: r["url"].as_str().unwrap_or("").to_string(),
                    snippet: r["description"].as_str().unwrap_or("").to_string(),
                    source: "Firecrawl".to_string(),
                    relevance: 1.0 - (i as f32 * 0.05),
                });
            }
        }

        Ok(results)
    }

    async fn search_tavily(&self, query: &str) -> Result<Vec<SearchResult>> {
        if self.config.tavily_api_key.is_empty() {
            return self.search_duckduckgo(query).await;
        }

        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "api_key": self.config.tavily_api_key,
            "query": query,
            "search_depth": "basic",
            "max_results": self.config.max_results,
            "include_answer": true,
        });

        let resp = client.post("https://api.tavily.com/search")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await.unwrap_or_default();
        let mut results = Vec::new();

        if let Some(items) = json["results"].as_array() {
            for (i, r) in items.iter().enumerate() {
                results.push(SearchResult {
                    title: r["title"].as_str().unwrap_or("").to_string(),
                    url: r["url"].as_str().unwrap_or("").to_string(),
                    snippet: r["content"].as_str().unwrap_or("").to_string(),
                    source: "Tavily".to_string(),
                    relevance: 1.0 - (i as f32 * 0.05),
                });
            }
        }

        // Include Tavily's AI answer if available
        if let Some(answer) = json["answer"].as_str() {
            if !answer.is_empty() {
                results.insert(0, SearchResult {
                    title: format!("AI Answer: {}", &query[..query.len().min(60)]),
                    url: "https://tavily.com".to_string(),
                    snippet: answer.to_string(),
                    source: "Tavily AI".to_string(),
                    relevance: 1.0,
                });
            }
        }

        Ok(results)
    }

    async fn search_google(&self, query: &str) -> Result<Vec<SearchResult>> {
        // Fall back to DuckDuckGo (free, no API key needed)
        self.search_duckduckgo(query).await
    }
}
