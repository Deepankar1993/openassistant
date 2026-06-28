// src/cron/scheduler.rs
//! Persisted cron jobs (`<data_dir>/cron.json`). Job *detection* lives here;
//! *execution + delivery* is the proactive loop's job (it has the config,
//! agent, and `post_everywhere`). Schedules are simple `"every <N>{m,h,d}"`.
use anyhow::Result;
use chrono::{DateTime, Datelike, Local, Timelike, Utc};
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
                job.next_run = next_run_after(&job.schedule, now);
                due.push(job.clone());
            }
        }
        due
    }
}

/// Is a schedule due now? Supports three grammars, tried in order:
///   1. interval  — "every <N>{m,h,d}"            (unchanged, instant-based)
///   2. crontab   — 5-field "M H DoM Mon DoW"     (calendar, local time)
///   3. natural   — "every day at 9am" / "daily at 21:00"  → crontab
/// Unknown formats are never due (logged, as before).
pub fn schedule_due(schedule: &str, last_run: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    schedule_due_local(schedule, last_run, now.with_timezone(&Local))
}

/// Local-time core of `schedule_due`, split out so tests are deterministic
/// regardless of the host timezone (`last_run` stays an absolute instant, and
/// `signed_duration_since` is timezone-agnostic).
fn schedule_due_local(
    schedule: &str,
    last_run: Option<DateTime<Utc>>,
    now_local: DateTime<Local>,
) -> bool {
    // 1. Interval — preserved exactly.
    if let Some(interval) = parse_every(schedule) {
        return match last_run {
            None => true,
            Some(last) => now_local.signed_duration_since(last) >= interval,
        };
    }
    // 2/3. Calendar (crontab or natural language → crontab).
    if let Some(spec) = parse_schedule_spec(schedule) {
        if !spec.matches(now_local) {
            return false;
        }
        // Cron-daemon semantics: at most once per matching minute. The 60s tick
        // (proactive.rs) plus this guard prevents a double-fire inside the minute.
        return match last_run {
            None => true,
            Some(last) => now_local.signed_duration_since(last) >= chrono::Duration::seconds(60),
        };
    }
    warn!(
        "cron: unrecognized schedule '{}' (use 'every <N>m|h|d', a 5-field crontab, or 'every day at 9am')",
        schedule
    );
    false
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

/// Whether a schedule string is well-formed (interval, crontab, or NL). Used by
/// the CLI to reject bad input up front.
pub fn is_valid_schedule(schedule: &str) -> bool {
    parse_every(schedule).is_some() || parse_schedule_spec(schedule).is_some()
}

/// Next scheduled instant strictly after `after`, or None for unknown formats.
/// Used to populate `CronJob.next_run` for display.
pub fn next_run_after(schedule: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    next_run_after_local(schedule, after.with_timezone(&Local)).map(|t| t.with_timezone(&Utc))
}

fn next_run_after_local(schedule: &str, after_local: DateTime<Local>) -> Option<DateTime<Local>> {
    if let Some(interval) = parse_every(schedule) {
        return Some(after_local + interval);
    }
    let spec = parse_schedule_spec(schedule)?;
    // Scan minute-by-minute (cron has minute resolution). Start strictly after
    // `after_local`, zeroed to the minute. Bound at ~1 year so a never-matching
    // spec can't loop forever.
    let mut cand = (after_local + chrono::Duration::minutes(1))
        .with_second(0)
        .and_then(|t| t.with_nanosecond(0))?;
    for _ in 0..(366 * 24 * 60) {
        if spec.matches(cand) {
            return Some(cand);
        }
        cand += chrono::Duration::minutes(1);
    }
    None
}

/// Try crontab first, then a few natural-language forms that desugar to crontab.
fn parse_schedule_spec(schedule: &str) -> Option<CronSpec> {
    CronSpec::parse(schedule)
        .or_else(|| natural_to_cron(schedule).as_deref().and_then(CronSpec::parse))
}

/// "every day at 9am" / "daily at 9:30pm" / "every day at 21:00" → "M H * * *".
fn natural_to_cron(input: &str) -> Option<String> {
    let lower = input.trim().to_lowercase();
    let rest = lower
        .strip_prefix("every day at ")
        .or_else(|| lower.strip_prefix("daily at "))?
        .trim();
    let (h, m) = parse_clock(rest)?;
    Some(format!("{} {} * * *", m, h))
}

/// Parse "9am", "9:30pm", "21:00", "9" into (hour 0-23, minute 0-59).
fn parse_clock(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    let (body, ampm) = if let Some(b) = s.strip_suffix("am") {
        (b.trim(), Some(false))
    } else if let Some(b) = s.strip_suffix("pm") {
        (b.trim(), Some(true))
    } else {
        (s, None)
    };
    let (h_str, m_str) = match body.split_once(':') {
        Some((h, m)) => (h, m),
        None => (body, "0"),
    };
    let mut h: u32 = h_str.trim().parse().ok()?;
    let m: u32 = m_str.trim().parse().ok()?;
    match ampm {
        Some(true) => {
            if h != 12 {
                h += 12;
            }
        } // 12pm = 12, 1-11pm = +12
        Some(false) => {
            if h == 12 {
                h = 0;
            }
        } // 12am = 0
        None => {}
    }
    if h < 24 && m < 60 {
        Some((h, m))
    } else {
        None
    }
}

/// A parsed 5-field crontab spec. Each field is the expanded set of allowed
/// values; `*_star` records whether the source field was "*" (for the classic
/// day-of-month / day-of-week OR rule).
#[derive(Debug, Clone)]
struct CronSpec {
    minute: Vec<u32>,
    hour: Vec<u32>,
    dom: Vec<u32>,
    month: Vec<u32>,
    dow: Vec<u32>, // 0 = Sunday
    dom_star: bool,
    dow_star: bool,
}

impl CronSpec {
    fn parse(s: &str) -> Option<CronSpec> {
        let f: Vec<&str> = s.split_whitespace().collect();
        if f.len() != 5 {
            return None;
        }
        let minute = parse_field(f[0], 0, 59)?;
        let hour = parse_field(f[1], 0, 23)?;
        let dom = parse_field(f[2], 1, 31)?;
        let month = parse_field(f[3], 1, 12)?;
        let mut dow = parse_field(f[4], 0, 7)?;
        for d in dow.iter_mut() {
            if *d == 7 {
                *d = 0;
            } // crontab allows 7 = Sunday
        }
        Some(CronSpec {
            minute,
            hour,
            dom,
            month,
            dow,
            dom_star: f[2] == "*",
            dow_star: f[4] == "*",
        })
    }

    fn matches(&self, dt: DateTime<Local>) -> bool {
        let dow = dt.weekday().num_days_from_sunday(); // 0=Sun..6=Sat
        let dom_ok = self.dom.contains(&dt.day());
        let dow_ok = self.dow.contains(&dow);
        // Classic crontab rule: if BOTH dom and dow are restricted, OR them;
        // otherwise AND (a "*" field is always satisfied).
        let day_ok = if !self.dom_star && !self.dow_star {
            dom_ok || dow_ok
        } else {
            (self.dom_star || dom_ok) && (self.dow_star || dow_ok)
        };
        self.minute.contains(&dt.minute())
            && self.hour.contains(&dt.hour())
            && self.month.contains(&dt.month())
            && day_ok
    }
}

/// Expand one crontab field into its allowed values. Supports `*`, an integer,
/// `a-b` ranges, `*/step` or `a-b/step`, and comma lists of those. Returns None
/// on any malformed token (so `parse_schedule_spec` falls through cleanly).
fn parse_field(field: &str, min: u32, max: u32) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for part in field.split(',') {
        let part = part.trim();
        let (base, step) = match part.split_once('/') {
            Some((b, s)) => (b.trim(), s.trim().parse::<u32>().ok().filter(|n| *n > 0)?),
            None => (part, 1),
        };
        let (lo, hi) = if base == "*" {
            (min, max)
        } else if let Some((a, b)) = base.split_once('-') {
            (a.trim().parse().ok()?, b.trim().parse().ok()?)
        } else {
            let v: u32 = base.parse().ok()?;
            (v, v)
        };
        if lo > hi || lo < min || hi > max {
            return None;
        }
        let mut v = lo;
        while v <= hi {
            out.push(v);
            v += step;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
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
        // Unknown / malformed interval → never due. ("0 8 * * *" is now a valid
        // crontab, exercised by the calendar tests below.)
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

    use chrono::TimeZone;

    fn local(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 6, 12, h, m, 0).unwrap() // 2026-06-12 is a Friday
    }

    #[test]
    fn crontab_parse_and_match() {
        let spec = CronSpec::parse("0 9 * * *").unwrap();
        assert!(spec.matches(local(9, 0)));
        assert!(!spec.matches(local(9, 1)));
        assert!(!spec.matches(local(8, 0)));
        // step + list + range expand correctly
        assert!(CronSpec::parse("*/15 * * * *").unwrap().matches(local(10, 30)));
        assert!(!CronSpec::parse("*/15 * * * *").unwrap().matches(local(10, 7)));
        assert!(CronSpec::parse("0 9,17 * * *").unwrap().matches(local(17, 0)));
        assert!(CronSpec::parse("0 9 * * 1-5").unwrap().matches(local(9, 0))); // Fri in Mon-Fri
        assert!(!CronSpec::parse("0 9 * * 0").unwrap().matches(local(9, 0))); // Sun only
        // malformed → None (falls through to "never due")
        assert!(CronSpec::parse("0 9 * *").is_none()); // 4 fields
        assert!(CronSpec::parse("99 9 * * *").is_none()); // minute out of range
    }

    #[test]
    fn natural_language_desugars_to_crontab() {
        assert_eq!(natural_to_cron("every day at 9am").as_deref(), Some("0 9 * * *"));
        assert_eq!(natural_to_cron("daily at 9:30pm").as_deref(), Some("30 21 * * *"));
        assert_eq!(natural_to_cron("every day at 21:00").as_deref(), Some("0 21 * * *"));
        assert_eq!(natural_to_cron("every day at 12am").as_deref(), Some("0 0 * * *"));
        assert_eq!(natural_to_cron("every day at 12pm").as_deref(), Some("0 12 * * *"));
        assert!(natural_to_cron("at some point").is_none());
        assert!(parse_schedule_spec("every day at 9am").unwrap().matches(local(9, 0)));
    }

    #[test]
    fn calendar_schedule_due_fires_once_per_minute() {
        // Crontab "0 9 * * *": due at 09:00, not at 09:01, dedup within the minute.
        assert!(schedule_due_local("0 9 * * *", None, local(9, 0)));
        assert!(!schedule_due_local("0 9 * * *", None, local(9, 1)));
        let last = local(9, 0).with_timezone(&Utc);
        assert!(!schedule_due_local("0 9 * * *", Some(last), local(9, 0))); // already ran this minute
        assert!(schedule_due_local(
            "0 9 * * *",
            Some(last - chrono::Duration::days(1)),
            local(9, 0)
        )); // ran yesterday → due
        // NL form behaves identically.
        assert!(schedule_due_local("every day at 9am", None, local(9, 0)));
    }

    #[test]
    fn interval_grammar_is_preserved() {
        // Unchanged "every N{m,h,d}" path still works through schedule_due_local.
        assert!(schedule_due_local("every 30m", None, local(9, 0)));
        let last = local(9, 0).with_timezone(&Utc);
        assert!(!schedule_due_local(
            "every 30m",
            Some(last - chrono::Duration::minutes(20)),
            local(9, 0)
        ));
        assert!(schedule_due_local(
            "every 30m",
            Some(last - chrono::Duration::minutes(40)),
            local(9, 0)
        ));
        assert!(is_valid_schedule("every 30m"));
        assert!(is_valid_schedule("0 9 * * *"));
        assert!(is_valid_schedule("every day at 9am"));
        assert!(!is_valid_schedule("nonsense"));
    }

    #[test]
    fn next_run_after_computes_calendar_and_interval() {
        // From 08:00, next "0 9 * * *" is 09:00 the same day.
        let next = next_run_after_local("0 9 * * *", local(8, 0)).unwrap();
        assert_eq!((next.hour(), next.minute(), next.day()), (9, 0, 12));
        // From 09:30 (past today's slot), next is 09:00 tomorrow.
        let next = next_run_after_local("0 9 * * *", local(9, 30)).unwrap();
        assert_eq!((next.hour(), next.minute(), next.day()), (9, 0, 13));
        // Interval next-run is just +interval.
        let next = next_run_after_local("every 2h", local(9, 0)).unwrap();
        assert_eq!(next.hour(), 11);
    }
}
