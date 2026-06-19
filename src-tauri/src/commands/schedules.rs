//! Schedules commands: READ-ONLY listings of everything scheduled / proactive.
//!
//! Pulls from the REAL core (not stubs):
//! - cron jobs        → `CronScheduler::load(<data_dir>/cron.json)`
//! - standing orders  → `StandingOrdersEngine::load(<data_dir>/standing_orders.json)`
//! - watchers         → `WatcherStore::open(<data_dir>/proactive.json)`
//! - daily brief      → `config::BriefConfig`
//!
//! Data-dir resolution matches the other command modules: `config::load()` →
//! `cfg.general.data_dir`. Empty files / missing data dir → empty vecs /
//! defaults, never a hard error. No mutations in this batch — read-only.

use super::mask_key;
use open_assistant::config;
use open_assistant::core::standing_orders::{OrderAction, OrderTrigger, StandingOrder, StandingOrdersEngine};
use open_assistant::core::watchers::{Watcher, WatcherStore};
use open_assistant::cron::scheduler::{CronJob, CronScheduler};
use serde::Serialize;

/// Truncate a string to ~`max` characters, appending an ellipsis when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

async fn data_dir() -> Result<String, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    Ok(cfg.general.data_dir)
}

// ── Cron jobs ──

#[derive(Debug, Serialize)]
pub struct CronJobDto {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub prompt: String,
    pub enabled: bool,
    pub last_run: Option<String>,
}

impl From<&CronJob> for CronJobDto {
    fn from(j: &CronJob) -> Self {
        Self {
            id: j.id.clone(),
            name: j.name.clone(),
            schedule: j.schedule.clone(),
            // The core field is `task`; the frontend wants `prompt`.
            prompt: truncate(&j.task, 200),
            enabled: j.enabled,
            last_run: j.last_run.map(|t| t.to_rfc3339()),
        }
    }
}

/// Cron jobs from `<data_dir>/cron.json` (empty if absent/corrupt).
#[tauri::command(rename_all = "snake_case")]
pub async fn list_cron_jobs() -> Result<Vec<CronJobDto>, String> {
    let dd = data_dir().await?;
    let scheduler = CronScheduler::load(&dd);
    Ok(scheduler.list_jobs().into_iter().map(CronJobDto::from).collect())
}

// ── Standing orders ──

#[derive(Debug, Serialize)]
pub struct StandingOrderDto {
    pub id: String,
    pub name: String,
    pub trigger: String,
    pub action: String,
    pub enabled: bool,
}

/// Human-readable label for an order trigger.
fn trigger_label(t: &OrderTrigger) -> String {
    match t {
        OrderTrigger::Keyword { phrases } => format!("Keyword: {}", phrases.join(", ")),
        OrderTrigger::SessionEnd => "SessionEnd".to_string(),
        OrderTrigger::ToolUsed { tool_name } => format!("ToolUsed: {tool_name}"),
        OrderTrigger::Event { event } => format!("Event: {event}"),
        OrderTrigger::Schedule { cron } => format!("Schedule: {cron}"),
        OrderTrigger::OnBoot => "OnBoot".to_string(),
        OrderTrigger::EveryNMessages { count } => format!("EveryNMessages: {count}"),
    }
}

/// Human-readable label for an order action (the variant name, e.g. "RunCommand").
fn action_label(a: &OrderAction) -> String {
    match a {
        OrderAction::InjectContext { .. } => "InjectContext",
        OrderAction::SaveNote { .. } => "SaveNote",
        OrderAction::RunTool { .. } => "RunTool",
        OrderAction::SendMessage { .. } => "SendMessage",
        OrderAction::RunSkill { .. } => "RunSkill",
        OrderAction::RunCommand { .. } => "RunCommand",
        OrderAction::Webhook { .. } => "Webhook",
    }
    .to_string()
}

impl From<&StandingOrder> for StandingOrderDto {
    fn from(o: &StandingOrder) -> Self {
        Self {
            id: o.id.clone(),
            name: o.name.clone(),
            trigger: trigger_label(&o.trigger),
            action: action_label(&o.action),
            enabled: o.enabled,
        }
    }
}

/// Standing orders from `<data_dir>/standing_orders.json` (defaults if absent).
#[tauri::command(rename_all = "snake_case")]
pub async fn list_standing_orders() -> Result<Vec<StandingOrderDto>, String> {
    let dd = data_dir().await?;
    let engine = StandingOrdersEngine::load(&dd);
    Ok(engine.list().iter().map(StandingOrderDto::from).collect())
}

// ── Watchers ──

#[derive(Debug, Serialize)]
pub struct WatcherDto {
    pub id: String,
    pub url: String,
    pub note: String,
    pub last_checked: Option<String>,
}

impl From<&Watcher> for WatcherDto {
    fn from(w: &Watcher) -> Self {
        Self {
            id: w.id.clone(),
            url: w.url.clone(),
            note: w.note.clone(),
            // Watcher stores RFC3339 strings; empty = never checked.
            last_checked: if w.last_checked.trim().is_empty() {
                None
            } else {
                Some(w.last_checked.clone())
            },
        }
    }
}

/// URL watchers from `<data_dir>/proactive.json` (empty if absent).
#[tauri::command(rename_all = "snake_case")]
pub async fn list_watchers() -> Result<Vec<WatcherDto>, String> {
    let dd = data_dir().await?;
    let store = WatcherStore::open(&dd);
    Ok(store.state.watchers.iter().map(WatcherDto::from).collect())
}

// ── Daily brief ──

#[derive(Debug, Serialize)]
pub struct BriefDto {
    pub enabled: bool,
    pub time: String,
    pub discord: bool,
    /// Masked when non-empty (it can identify a chat); empty stays empty.
    pub telegram_chat_id: String,
}

/// Daily-brief settings from config (`[brief]`).
#[tauri::command(rename_all = "snake_case")]
pub async fn get_brief_settings() -> Result<BriefDto, String> {
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let b = cfg.brief;
    Ok(BriefDto {
        enabled: b.enabled,
        time: b.time,
        discord: b.discord,
        telegram_chat_id: if b.telegram_chat_id.trim().is_empty() {
            String::new()
        } else {
            mask_key(&b.telegram_chat_id)
        },
    })
}

// ── Bundled overview ──

#[derive(Debug, Serialize)]
pub struct SchedulesOverviewDto {
    pub cron: Vec<CronJobDto>,
    pub standing_orders: Vec<StandingOrderDto>,
    pub watchers: Vec<WatcherDto>,
    pub brief: BriefDto,
}

/// Everything scheduled / proactive in one call (what the frontend likely uses).
#[tauri::command(rename_all = "snake_case")]
pub async fn get_schedules_overview() -> Result<SchedulesOverviewDto, String> {
    Ok(SchedulesOverviewDto {
        cron: list_cron_jobs().await?,
        standing_orders: list_standing_orders().await?,
        watchers: list_watchers().await?,
        brief: get_brief_settings().await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_assistant::core::standing_orders::{OrderAction, OrderTrigger, StandingOrder};
    use open_assistant::core::watchers::Watcher;
    use open_assistant::cron::scheduler::CronJob;

    // `chrono` isn't a direct dep of the desktop crate, so we build `CronJob`
    // (whose `last_run` is `Option<DateTime<Utc>>`) from JSON via serde rather
    // than constructing chrono types directly.
    fn cron_job_from_json(task: &str, last_run: serde_json::Value) -> CronJob {
        serde_json::from_value(serde_json::json!({
            "id": "abc123",
            "name": "daily",
            "schedule": "every 1d",
            "task": task,
            "enabled": true,
            "last_run": last_run,
            "next_run": null,
            "delivery_target": null,
            "run_count": 3,
        }))
        .expect("CronJob deserializes")
    }

    #[test]
    fn cron_job_dto_maps_fields_and_truncates_prompt() {
        let long = "x".repeat(500);
        let job = cron_job_from_json(&long, serde_json::json!("2026-06-18T08:00:00Z"));
        let dto = CronJobDto::from(&job);
        assert_eq!(dto.id, "abc123");
        assert_eq!(dto.name, "daily");
        assert_eq!(dto.schedule, "every 1d");
        assert!(dto.enabled);
        // Truncated to 200 chars + the ellipsis.
        assert_eq!(dto.prompt.chars().count(), 201);
        assert!(dto.prompt.ends_with('…'));
        // RFC3339 round-trip (UTC renders the offset as +00:00).
        assert_eq!(dto.last_run.as_deref(), Some("2026-06-18T08:00:00+00:00"));
    }

    #[test]
    fn cron_job_dto_keeps_short_prompt_and_none_last_run() {
        let job = cron_job_from_json("summarize my day", serde_json::Value::Null);
        let dto = CronJobDto::from(&job);
        assert_eq!(dto.prompt, "summarize my day");
        assert!(dto.last_run.is_none());
    }

    #[test]
    fn standing_order_dto_maps_trigger_and_action_labels() {
        let order = StandingOrder {
            id: "o1".into(),
            name: "deploy-watch".into(),
            enabled: true,
            trigger: OrderTrigger::Keyword { phrases: vec!["deploy".into(), "ship".into()] },
            action: OrderAction::RunCommand { command: "echo hi".into() },
            description: String::new(),
        };
        let dto = StandingOrderDto::from(&order);
        assert_eq!(dto.id, "o1");
        assert_eq!(dto.name, "deploy-watch");
        assert_eq!(dto.trigger, "Keyword: deploy, ship");
        assert_eq!(dto.action, "RunCommand");
        assert!(dto.enabled);
    }

    #[test]
    fn standing_order_action_and_trigger_labels_cover_variants() {
        assert_eq!(action_label(&OrderAction::InjectContext { text: "t".into() }), "InjectContext");
        assert_eq!(action_label(&OrderAction::SaveNote { template: "t".into() }), "SaveNote");
        assert_eq!(action_label(&OrderAction::Webhook { url: "u".into(), body: "b".into() }), "Webhook");
        assert_eq!(trigger_label(&OrderTrigger::SessionEnd), "SessionEnd");
        assert_eq!(trigger_label(&OrderTrigger::EveryNMessages { count: 3 }), "EveryNMessages: 3");
        assert_eq!(trigger_label(&OrderTrigger::OnBoot), "OnBoot");
    }

    #[test]
    fn watcher_dto_maps_fields_and_optional_last_checked() {
        let checked = Watcher {
            id: "w1".into(),
            url: "https://example.com".into(),
            note: "release page".into(),
            interval_minutes: 30,
            last_hash: String::new(),
            last_checked: "2026-06-18T08:00:00+00:00".into(),
            last_changed: String::new(),
        };
        let dto = WatcherDto::from(&checked);
        assert_eq!(dto.id, "w1");
        assert_eq!(dto.url, "https://example.com");
        assert_eq!(dto.note, "release page");
        assert_eq!(dto.last_checked.as_deref(), Some("2026-06-18T08:00:00+00:00"));

        let never = Watcher {
            id: "w2".into(),
            url: "https://b.example".into(),
            note: String::new(),
            interval_minutes: 30,
            last_hash: String::new(),
            last_checked: String::new(),
            last_changed: String::new(),
        };
        assert!(WatcherDto::from(&never).last_checked.is_none());
    }

    #[test]
    fn brief_masks_telegram_chat_id_when_present() {
        // Non-empty is masked (no clear value leaks); the empty case stays empty.
        let masked = mask_key("123456789");
        assert!(masked.ends_with("6789"));
        assert!(!masked.contains("12345"));
        assert_eq!(mask_key(""), "");
    }

    #[test]
    fn truncate_counts_chars_not_bytes() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
        // Multibyte safety: 4 chars kept, no panic on a char boundary.
        assert_eq!(truncate("héllo wörld", 4), "héll…");
    }
}
