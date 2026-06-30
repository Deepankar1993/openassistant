//! Tauri command surface bridging the frontend to the `open_assistant` core.
//!
//! Commands NEVER reproduce the hardcoded "simulated response" placeholders that
//! `ui::web.rs` / `ui::tui.rs` return — they call the real core. Organized by
//! domain so the growing surface stays navigable (openspec change
//! `add-desktop-onboarding-options`, task 1.8).

pub mod chat;
pub mod gateway;
pub mod memory;
pub mod onboarding;
pub mod persona;
pub mod schedules;
pub mod settings;
pub mod skills;
pub mod system;
pub mod updater;

/// Mask a secret (API key / channel token): keep only the last 4 characters.
/// Shared by the Settings and onboarding DTOs so nothing is ever sent in clear.
pub(crate) fn mask_key(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() {
        return String::new();
    }
    let visible: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••••••{visible}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_hides_all_but_last_four() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key("   "), "");
        let masked = mask_key("sk-or-v1-ABCD1234");
        assert!(masked.ends_with("1234"));
        assert!(masked.starts_with("••••••••"));
        assert!(!masked.contains("ABCD"));
    }
}
