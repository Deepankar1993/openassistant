// src/security/allowlist.rs
use anyhow::Result;

pub fn is_allowed(user_id: &str, allowlist: &[String]) -> bool {
    if allowlist.is_empty() {
        return true; // No restrictions
    }
    allowlist.iter().any(|entry| entry == user_id || entry == "*")
}
