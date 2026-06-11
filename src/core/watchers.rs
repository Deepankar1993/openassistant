// src/core/watchers.rs
//! URL watchers — "watch this page, tell me when it changes".
//!
//! State lives in `<data_dir>/proactive.json` (atomic JSON writes, the
//! goal-store pattern). The same file carries `last_brief_date` so all
//! proactive-loop state is in one place. Watchers are managed by the `watch`
//! agent tool and checked by the gateway's proactive loop.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Minimum allowed check interval — protects watched sites and our tick loop.
pub const MIN_INTERVAL_MINUTES: u64 = 5;
/// Cap fetched bodies so a huge page can't balloon memory.
const MAX_BODY_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watcher {
    pub id: String,
    pub url: String,
    /// User's note about why they're watching ("price of X", "release page").
    #[serde(default)]
    pub note: String,
    pub interval_minutes: u64,
    /// Hash of the last seen (whitespace-normalized) body; empty = never fetched.
    #[serde(default)]
    pub last_hash: String,
    /// RFC3339; empty = never checked.
    #[serde(default)]
    pub last_checked: String,
    /// RFC3339 of the last detected change; empty = none yet.
    #[serde(default)]
    pub last_changed: String,
}

/// One watcher's change, ready to be turned into a notification.
#[derive(Debug, Clone)]
pub struct WatcherChange {
    pub url: String,
    pub note: String,
    /// The new (normalized, truncated) body text — input for an LLM summary.
    pub body: String,
    /// True the first time a watcher is fetched (baseline, not a change).
    pub first_fetch: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProactiveState {
    #[serde(default)]
    pub watchers: Vec<Watcher>,
    /// YYYY-MM-DD (local) of the last delivered daily brief.
    #[serde(default)]
    pub last_brief_date: String,
}

pub struct WatcherStore {
    path: PathBuf,
    pub state: ProactiveState,
}

impl WatcherStore {
    pub fn open(data_dir: &str) -> Self {
        let path = PathBuf::from(data_dir).join("proactive.json");
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, state }
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.state)?;
        let tmp = tempfile::NamedTempFile::new_in(
            self.path.parent().unwrap_or_else(|| std::path::Path::new(".")),
        )?;
        std::fs::write(tmp.path(), json)?;
        tmp.persist(&self.path)?;
        Ok(())
    }

    pub fn add(&mut self, url: &str, note: &str, interval_minutes: u64) -> Result<String> {
        let interval = interval_minutes.max(MIN_INTERVAL_MINUTES);
        let id = uuid::Uuid::new_v4().to_string();
        self.state.watchers.push(Watcher {
            id: id.clone(),
            url: url.to_string(),
            note: note.to_string(),
            interval_minutes: interval,
            last_hash: String::new(),
            last_checked: String::new(),
            last_changed: String::new(),
        });
        self.save()?;
        Ok(id)
    }

    /// Remove by id prefix or exact URL. Returns whether anything was removed.
    pub fn remove(&mut self, key: &str) -> Result<bool> {
        let before = self.state.watchers.len();
        self.state.watchers.retain(|w| !(w.id.starts_with(key) || w.url == key));
        let removed = self.state.watchers.len() != before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn format_list(&self) -> String {
        if self.state.watchers.is_empty() {
            return "No watchers configured. Add one with action=add.".to_string();
        }
        let mut out = format!("{} watcher(s):\n", self.state.watchers.len());
        for w in &self.state.watchers {
            out.push_str(&format!(
                "- [{}] {} — every {}m{}{}\n",
                &w.id[..8.min(w.id.len())],
                w.url,
                w.interval_minutes,
                if w.note.is_empty() { String::new() } else { format!(" ({})", w.note) },
                if w.last_changed.is_empty() {
                    String::new()
                } else {
                    format!(" — last change {}", w.last_changed)
                },
            ));
        }
        out
    }

    /// Indices of watchers due for a check at `now`.
    pub fn due_indices(&self, now: chrono::DateTime<chrono::Utc>) -> Vec<usize> {
        self.state
            .watchers
            .iter()
            .enumerate()
            .filter(|(_, w)| match chrono::DateTime::parse_from_rfc3339(&w.last_checked) {
                Ok(t) => now.signed_duration_since(t.with_timezone(&chrono::Utc))
                    >= chrono::Duration::minutes(w.interval_minutes as i64),
                Err(_) => true, // never checked
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Fetch every due watcher, update state, persist once, and return the
    /// changes (excluding first-fetch baselines). Fetch errors leave
    /// `last_hash` untouched so an outage never reads as a change. Each fetch
    /// uses its own SSRF-hardened client (see `fetch_text`).
    pub async fn check_due(
        &mut self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<WatcherChange>> {
        let mut changes = Vec::new();
        for i in self.due_indices(now) {
            let url = self.state.watchers[i].url.clone();
            let body = match fetch_text(&url).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("watcher fetch failed for {}: {}", url, e);
                    self.state.watchers[i].last_checked = now.to_rfc3339();
                    continue;
                }
            };
            let normalized = normalize_body(&body);
            let hash = content_hash(&normalized);
            let w = &mut self.state.watchers[i];
            let first_fetch = w.last_hash.is_empty();
            let changed = !first_fetch && w.last_hash != hash;
            w.last_checked = now.to_rfc3339();
            if first_fetch || changed {
                w.last_hash = hash;
            }
            if changed {
                w.last_changed = now.to_rfc3339();
                changes.push(WatcherChange {
                    url,
                    note: w.note.clone(),
                    body: normalized.chars().take(3000).collect(),
                    first_fetch: false,
                });
            }
        }
        self.save()?;
        Ok(changes)
    }
}

/// SSRF-hardened fetch. Watchers can be added by remote channel users and the
/// fetched content is LLM-summarized back to the channel, so this must not be
/// usable to read internal services or cloud metadata endpoints:
/// - http/https schemes only;
/// - the host string is checked (localhost / IP-literal ranges), then every
///   DNS-resolved address is checked too (catches public hostnames pointing
///   at internal IPs);
/// - the connection is pinned to a validated resolved IP via
///   `ClientBuilder::resolve`, so a DNS rebind between check and connect
///   changes nothing;
/// - redirects are not followed (a public URL could 302 to an internal one) —
///   watch the final URL directly instead.
async fn fetch_text(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("only http(s) URLs can be watched");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL has no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if is_internal_host(&host) {
        anyhow::bail!("private/localhost URLs are not allowed for watchers");
    }
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<std::net::SocketAddr> =
        tokio::net::lookup_host((host.as_str(), port)).await?.collect();
    if addrs.is_empty() {
        anyhow::bail!("DNS returned no addresses for {host}");
    }
    if addrs.iter().any(|a| is_internal_ip(&a.ip())) {
        anyhow::bail!("URL resolves to a private/internal address");
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .resolve(&host, addrs[0])
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client
        .get(url)
        .header("User-Agent", "openAssistant-watcher/0.1")
        .send()
        .await?;
    let status = resp.status();
    if status.is_redirection() {
        anyhow::bail!("redirects are not followed for watched URLs (HTTP {status}) — watch the final URL directly");
    }
    if !status.is_success() {
        anyhow::bail!("HTTP {status}");
    }
    // Enforce the byte cap on raw bytes (a char-based cap would let CJK
    // pages use 4x the intended memory).
    let bytes = resp.bytes().await?;
    let truncated = &bytes[..bytes.len().min(MAX_BODY_BYTES)];
    Ok(String::from_utf8_lossy(truncated).into_owned())
}

/// True for hosts a watcher must not fetch: localhost, loopback, RFC1918
/// private ranges, and link-local (incl. cloud metadata at 169.254.169.254).
/// Hostname-only check — `fetch_text` additionally validates every resolved
/// address with `is_internal_ip`.
fn is_internal_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") || host.is_empty() {
        return true;
    }
    // Bracketed IPv6 hosts parse without the brackets.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return is_internal_ip(&ip);
    }
    false
}

fn is_internal_ip(ip: &std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(a) => {
            a.is_loopback() || a.is_private() || a.is_link_local() || a.is_unspecified()
        }
        IpAddr::V6(a) => {
            // Also catch internal IPv4 addresses smuggled as v6-mapped (::ffff:10.0.0.1).
            if let Some(v4) = a.to_ipv4_mapped() {
                return is_internal_ip(&IpAddr::V4(v4));
            }
            a.is_loopback()
                || a.is_unspecified()
                // fe80::/10 link-local and fc00::/7 unique-local
                || (a.segments()[0] & 0xffc0) == 0xfe80
                || (a.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// Collapse all whitespace runs so formatting/indentation churn doesn't read
/// as a content change.
fn normalize_body(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn content_hash(normalized: &str) -> String {
    hex::encode(Sha256::digest(normalized.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, WatcherStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = WatcherStore::open(dir.path().to_str().unwrap());
        (dir, store)
    }

    #[test]
    fn add_list_remove_round_trip() {
        let (dir, mut store) = temp_store();
        let id = store.add("https://example.com", "release page", 30).unwrap();
        assert!(store.format_list().contains("https://example.com"));
        assert!(store.format_list().contains("every 30m"));

        // Reopen from disk — persistence round-trip.
        let mut store2 = WatcherStore::open(dir.path().to_str().unwrap());
        assert_eq!(store2.state.watchers.len(), 1);
        assert!(store2.remove(&id[..8]).unwrap());
        assert!(store2.state.watchers.is_empty());

        let store3 = WatcherStore::open(dir.path().to_str().unwrap());
        assert!(store3.state.watchers.is_empty());
    }

    #[test]
    fn interval_is_clamped_to_minimum() {
        let (_dir, mut store) = temp_store();
        store.add("https://example.com", "", 1).unwrap();
        assert_eq!(store.state.watchers[0].interval_minutes, MIN_INTERVAL_MINUTES);
    }

    #[test]
    fn content_hash_ignores_whitespace_churn() {
        let a = content_hash(&normalize_body("hello   world\n\n  foo"));
        let b = content_hash(&normalize_body("hello world foo"));
        assert_eq!(a, b);
        let c = content_hash(&normalize_body("hello world bar"));
        assert_ne!(a, c);
    }

    #[test]
    fn internal_hosts_are_rejected() {
        assert!(is_internal_host("localhost"));
        assert!(is_internal_host("127.0.0.1"));
        assert!(is_internal_host("10.0.0.5"));
        assert!(is_internal_host("172.16.0.1"));
        assert!(is_internal_host("192.168.1.1"));
        assert!(is_internal_host("169.254.169.254")); // cloud metadata
        assert!(is_internal_host("0.0.0.0"));
        assert!(is_internal_host("[::1]"));
        assert!(is_internal_host("[fe80::1]"));
        assert!(!is_internal_host("93.184.216.34"));
        assert!(!is_internal_host("example.com"));
        // v4-mapped-in-v6 smuggling
        assert!(is_internal_host("[::ffff:10.0.0.1]"));
        assert!(is_internal_host("[::ffff:127.0.0.1]"));
    }

    #[tokio::test]
    async fn fetch_text_rejects_bad_targets_before_any_network() {
        // Non-http scheme.
        assert!(fetch_text("ftp://example.com/x").await.is_err());
        // Internal hosts bail before DNS/connect.
        assert!(fetch_text("http://127.0.0.1/x").await.is_err());
        assert!(fetch_text("http://169.254.169.254/latest/meta-data/").await.is_err());
        assert!(fetch_text("http://localhost:8080/admin").await.is_err());
    }

    #[test]
    fn due_selection_respects_interval() {
        let (_dir, mut store) = temp_store();
        store.add("https://a.example", "", 30).unwrap();
        store.add("https://b.example", "", 30).unwrap();
        let now = chrono::Utc::now();

        // Never checked → both due.
        assert_eq!(store.due_indices(now).len(), 2);

        // First checked 10 minutes ago (not due), second 31 minutes ago (due).
        store.state.watchers[0].last_checked = (now - chrono::Duration::minutes(10)).to_rfc3339();
        store.state.watchers[1].last_checked = (now - chrono::Duration::minutes(31)).to_rfc3339();
        let due = store.due_indices(now);
        assert_eq!(due, vec![1]);
    }
}
