// src/cron/mod.rs
pub mod scheduler;

use anyhow::Result;

pub async fn check() -> Result<usize> {
    let scheduler = scheduler::CronScheduler::new();
    Ok(scheduler.job_count())
}
