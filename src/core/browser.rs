// src/core/browser.rs
//! Real browser control — OpenClaw-style dedicated Chrome profile
//! Controls a real Chromium/Chrome/Edge/Brave browser via CDP (Chrome DevTools Protocol)

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, debug};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    pub profile_name: String,
    pub browser_cmd: String,
    pub user_data_dir: PathBuf,
    pub headless: bool,
    pub port: u16,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            profile_name: "openassistant".to_string(),
            browser_cmd: detect_browser(),
            user_data_dir: default_profile_dir(),
            headless: false,
            port: 9222,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Browser {
    config: BrowserConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSnapshot {
    pub url: String,
    pub title: String,
    pub content: String,
    pub tabs: Vec<TabInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub active: bool,
}

impl Browser {
    pub fn new(config: BrowserConfig) -> Self {
        Self { config }
    }

    pub fn default() -> Self {
        Self { config: BrowserConfig::default() }
    }

    pub async fn launch(&self) -> Result<()> {
        let profile = self.config.user_data_dir.to_string_lossy().to_string();
        let port = self.config.port;
        info!("Launching {} with profile '{}' on port {}", self.config.browser_cmd, self.config.profile_name, port);
        let cmd = format!("{} --remote-debugging-port={} --user-data-dir={} --no-first-run --no-default-browser-check 2>/dev/null &", self.config.browser_cmd, port, profile);
        let _ = tokio::task::spawn_blocking(move || {
            std::process::Command::new("bash").arg("-c").arg(&cmd).output()
        }).await;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        info!("Browser launched");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let port = self.config.port;
        let _ = tokio::task::spawn_blocking(move || {
            std::process::Command::new("bash").arg("-c").arg(format!("curl -s http://localhost:{}/json/quit 2>/dev/null || true", port)).output()
        }).await;
        info!("Browser stopped");
        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        let port = self.config.port;
        match tokio::task::spawn_blocking(move || {
            std::process::Command::new("bash").arg("-c").arg(format!("curl -s http://localhost:{}/json/version 2>/dev/null", port)).output()
        }).await {
            Ok(Ok(output)) => output.status.success(),
            _ => false,
        }
    }

    pub async fn list_tabs(&self) -> Result<Vec<TabInfo>> {
        let port = self.config.port;
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("bash").arg("-c").arg(format!("curl -s http://localhost:{}/json/list 2>/dev/null", port)).output()
        }).await.unwrap_or_else(|e| Ok(std::process::Output { status: std::process::ExitStatus::default(), stdout: Vec::new(), stderr: e.to_string().into_bytes() }))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let tabs: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap_or_default();
        let mut result = Vec::new();
        for (i, tab) in tabs.iter().enumerate() {
            result.push(TabInfo {
                id: tab["id"].as_str().unwrap_or("").to_string(),
                url: tab["url"].as_str().unwrap_or("").to_string(),
                title: tab["title"].as_str().unwrap_or("").to_string(),
                active: i == 0,
            });
        }
        Ok(result)
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        let port = self.config.port;
        let url_owned = url.to_string();
        tokio::task::spawn_blocking(move || {
            std::process::Command::new("bash").arg("-c").arg(format!("curl -s http://localhost:{}/json/new?{} 2>/dev/null", port, urlencoding::encode(&url_owned))).output()
        }).await?;
        info!("Navigated to: {}", url);
        Ok(())
    }

    pub async fn read_page(&self) -> Result<BrowserSnapshot> {
        let port = self.config.port;
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("bash")
                .arg("-c")
                .arg(format!("curl -s 'http://localhost:{}/json/list' 2>/dev/null | python3 -c \"import sys,json; tabs=json.load(sys.stdin); print(tabs[0]['title']+'\\n'+tabs[0]['url'] if tabs else '')\" 2>/dev/null", port))
                .output()
        }).await.unwrap_or_else(|e| Ok(std::process::Output { status: std::process::ExitStatus::default(), stdout: Vec::new(), stderr: e.to_string().into_bytes() }))?;

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        let mut lines = content.lines();
        let title = lines.next().unwrap_or("").to_string();
        let url = lines.next().unwrap_or("").to_string();

        Ok(BrowserSnapshot {
            url: url.clone(),
            title: title.clone(),
            content: format!("Title: {}\nURL: {}", title, url),
            tabs: vec![],
        })
    }

    pub async fn screenshot(&self, output_path: &str) -> Result<()> {
        let port = self.config.port;
        let path = output_path.to_string();
        tokio::task::spawn_blocking(move || {
            std::process::Command::new("bash")
                .arg("-c")
                .arg(format!("curl -s http://localhost:{}/json/list | python3 -c \"import sys,json,urllib.request; tabs=json.load(sys.stdin); print(tabs[0]['webSocketDebuggerUrl'] if tabs else '')\" 2>/dev/null | xargs -I{{}} python3 -c \"import websocket,json; ws=websocket.create_connection('{{}}'); ws.send(json.dumps({{'id':1,'method':'Page.captureScreenshot','params':{{'format':'png'}}}})); r=json.loads(ws.recv()); open('{}','wb').write(bytes(r['result']['data'])); ws.close()\" 2>/dev/null", port, path))
                .output()
        }).await?;
        Ok(())
    }

    pub async fn execute_js(&self, script: &str) -> Result<String> {
        let port = self.config.port;
        let script = script.replace('\'', "\\'");
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("bash")
                .arg("-c")
                .arg(format!("curl -s http://localhost:{}/json/list | python3 -c \"import sys,json,websocket; tabs=json.load(sys.stdin); ws=tabs[0]['webSocketDebuggerUrl']; conn=websocket.create_connection(ws); conn.send(json.dumps({{'id':1,'method':'Runtime.evaluate','params':{{'expression':'{}','returnByValue':True}}}})); r=json.loads(conn.recv()); conn.close(); print(r.get('result',{{}}).get('result',{{}}).get('value',''))\" 2>/dev/null", port, script))
                .output()
        }).await.unwrap_or_else(|e| Ok(std::process::Output { status: std::process::ExitStatus::default(), stdout: Vec::new(), stderr: e.to_string().into_bytes() }))?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

fn detect_browser() -> String {
    for cmd in &["brave-browser-stable", "brave-browser", "brave",
                 "google-chrome-stable", "google-chrome", "chromium-browser", "chromium"] {
        if std::process::Command::new("which").arg(cmd).output().map(|o| o.status.success()).unwrap_or(false) {
            return cmd.to_string();
        }
    }
    "chromium".to_string()
}

fn default_profile_dir() -> PathBuf {
    let data_dir = crate::config::data_dir_default();
    PathBuf::from(format!("{}/browser/{}", data_dir, "openassistant"))
}
