// src/cron/scheduler.rs
//! Persisted cron jobs (`<data_dir>/cron.json`). Job *detection* lives here;
//! *execution + delivery* is the proactive loop's job (it has the config,
//! agent, and `post_everywhere`). Schedules are simple `"every <N>{m,h,d}"`.
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub task: String,
    pub enabled: bool,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub delivery_target: Option<String>,
    pub run_count: u64,
}

#[derive(Debug)]
pub struct CronScheduler {
    jobs: HashMap<String, CronJob>,
}

impl CronScheduler {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }

    fn store_path(data_dir: &str) -> PathBuf {
        PathBuf::from(data_dir).join("cron.json")
    }

    /// Load jobs from `<data_dir>/cron.json` (empty scheduler if absent/corrupt).
    pub fn load(data_dir: &str) -> Self {
        let path = Self::store_path(data_dir);
        match std::fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str::<Vec<CronJob>>(&s) {
                Ok(jobs) => Self { jobs: jobs.into_iter().map(|j| (j.id.clone(), j)).collect() },
                Err(e) => {
                    warn!("cron.json unreadable ({e}); starting empty");
                    Self::new()
                }
            },
            Err(_) => Self::new(),
        }
    }

    pub fn save(&self, data_dir: &str) -> Result<()> {
        let path = Self::store_path(data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let jobs: Vec<&CronJob> = self.jobs.values().collect();
        let json = serde_json::to_string_pretty(&jobs)?;
        let tmp = tempfile::NamedTempFile::new_in(
            path.parent().unwrap_or_else(|| std::path::Path::new(".")),
        )?;
        std::fs::write(tmp.path(), json)?;
        tmp.persist(&path)?;
        Ok(())
    }

    pub fn add_job(&mut self, name: &str, schedule: &str, task: &str, delivery_target: Option<String>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let job = CronJob {
            id: id.clone(),
            name: name.to_string(),
            schedule: schedule.to_string(),
            task: task.to_string(),
            enabled: true,
            last_run: None,
            next_run: None,
            delivery_target,
            run_count: 0,
        };
        info!("Added cron job: {} ({})", name, schedule);
        self.jobs.insert(id.clone(), job);
        id
    }

    /// Remove by full id or id-prefix (so the truncated id from `list` works).
    pub fn remove_job(&mut self, id: &str) -> bool {
        let before = self.jobs.len();
        self.jobs.retain(|jid, _| !(jid == id || jid.starts_with(id)));
        self.jobs.len() < before
    }

    pub fn list_jobs(&self) -> Vec<&CronJob> {
        self.jobs.values().collect()
    }

    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }

    pub fn enable_job(&mut self, id: &str, enabled: bool) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.enabled = enabled;
        }
    }

    /// Return the enabled jobs that are due at `now`, marking each as run
    /// (`last_run = now`, `run_count += 1`). The caller executes + delivers and
    /// then persists the scheduler.
    pub fn take_due(&mut self, now: DateTime<Utc>) -> Vec<CronJob> {
        let due_ids: Vec<String> = self
            .jobs
            .values()
            .filter(|j| j.enabled && schedule_due(&j.schedule, j.last_run, now))
            .map(|j| j.id.clone())
            .collect();
        let mut due = Vec::new();
        for id in due_ids {
            if let Some(job) = self.jobs.get_mut(&id) {
                job.last_run = Some(now);
                job.run_count += 1;
                due.push(job.clone());
            }
        }
        due
    }
}

/// Is a `"every <N>{m,h,d}"` schedule due, given its last run and the current
/// time? Never-run jobs are due immediately. Unknown formats are never due.
pub fn schedule_due(schedule: &str, last_run: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let Some(interval) = parse_every(schedule) else {
        warn!("cron: unrecognized schedule '{}' (use 'every <N>m|h|d')", schedule);
        return false;
    };
    match last_run {
        None => true,
        Some(last) => now.signed_duration_since(last) >= interval,
    }
}

fn parse_every(schedule: &str) -> Option<chrono::Duration> {
    let rest = schedule.trim().strip_prefix("every ")?.trim();
    // Peel the last CHARACTER as the unit — `split_at(len-1)` would panic on a
    // multibyte final char (e.g. "every 5µ"), and the schedule is user input.
    let (unit_start, unit) = rest.char_indices().next_back()?;
    let n: i64 = rest[..unit_start].trim().parse().ok()?;
    if n <= 0 {
        return None;
    }
    match unit {
        'm' => Some(chrono::Duration::minutes(n)),
        'h' => Some(chrono::Duration::hours(n)),
        'd' => Some(chrono::Duration::days(n)),
        _ => None,
    }
}

/// Whether a schedule string is well-formed (`"every <N>{m,h,d}"`). Used by the
/// CLI to reject bad input up front.
pub fn is_valid_schedule(schedule: &str) -> bool {
    parse_every(schedule).is_some()
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_due_parses_units_and_boundaries() {
        let now = Utc::now();
        // Never run → due.
        assert!(schedule_due("every 60m", None, now));
        // 30m schedule, last run 20m ago → not due; 40m ago → due.
        assert!(!schedule_due("every 30m", Some(now - chrono::Duration::minutes(20)), now));
        assert!(schedule_due("every 30m", Some(now - chrono::Duration::minutes(40)), now));
        // hours / days units.
        assert!(schedule_due("every 2h", Some(now - chrono::Duration::hours(3)), now));
        assert!(!schedule_due("every 2h", Some(now - chrono::Duration::hours(1)), now));
        assert!(schedule_due("every 1d", Some(now - chrono::Duration::hours(25)), now));
        // Unknown / malformed → never due.
        assert!(!schedule_due("0 8 * * *", None, now));
        assert!(!schedule_due("every 0m", None, now));
        assert!(!schedule_due("nonsense", None, now));
        // Multibyte final char must NOT panic (was a split_at hazard).
        assert!(!schedule_due("every 5µ", None, now));
        assert!(!schedule_due("every ☃", None, now));
        assert!(is_valid_schedule("every 30m") && !is_valid_schedule("every 5µ"));
    }

    #[test]
    fn take_due_marks_and_does_not_refire_immediately() {
        let mut s = CronScheduler::new();
        s.add_job("a", "every 60m", "do a", None);
        s.add_job("b", "every 60m", "do b", None);
        let now = Utc::now();
        let due = s.take_due(now);
        assert_eq!(due.len(), 2, "both never-run jobs are due");
        assert!(due.iter().all(|j| j.run_count == 1));
        // Immediately again → nothing due (just marked last_run = now).
        assert!(s.take_due(now).is_empty());
    }

    #[test]
    fn persistence_round_trip_and_remove_by_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let dd = dir.path().to_str().unwrap();
        let mut s = CronScheduler::load(dd); // empty
        let id = s.add_job("daily", "every 1d", "summarize my day", None);
        s.save(dd).unwrap();

        let mut s2 = CronScheduler::load(dd);
        assert_eq!(s2.job_count(), 1);
        assert!(s2.remove_job(&id[..8]), "remove by 8-char prefix");
        assert_eq!(s2.job_count(), 0);
    }
}
