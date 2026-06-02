// src/cron/scheduler.rs
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, debug};

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

    pub fn add_job(&mut self, name: &str, schedule: &str, task: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let job = CronJob {
            id: id.clone(),
            name: name.to_string(),
            schedule: schedule.to_string(),
            task: task.to_string(),
            enabled: true,
            last_run: None,
            next_run: None,
            delivery_target: None,
            run_count: 0,
        };
        info!("Added cron job: {} ({})", name, schedule);
        self.jobs.insert(id.clone(), job);
        id
    }

    pub fn remove_job(&mut self, id: &str) -> bool {
        self.jobs.remove(id).is_some()
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

    pub async fn tick(&mut self) -> Vec<(String, Result<String>)> {
        let mut results = Vec::new();
        let now = Utc::now();

        let due_jobs: Vec<(String, String)> = self
            .jobs
            .values()
            .filter(|job| {
                if !job.enabled {
                    return false;
                }
                job.last_run.map_or(true, |last| {
                    let elapsed = now.signed_duration_since(last);
                    match job.schedule.as_str() {
                        s if s.starts_with("every ") => {
                            let mins: i64 = s
                                .trim_start_matches("every ")
                                .trim_end_matches('m')
                                .parse()
                                .unwrap_or(60);
                            elapsed.num_minutes() >= mins
                        }
                        _ => false,
                    }
                })
            })
            .map(|job| (job.id.clone(), job.task.clone()))
            .collect();

        for (id, task) in due_jobs {
            if let Some(job) = self.jobs.get_mut(&id) {
                debug!("Running cron job: {}", job.name);
                job.last_run = Some(now);
                job.run_count += 1;
            }

            let task_clone = task.clone();
            let result = self.execute_task(&task_clone).await;
            results.push((task.clone(), result));
        }

        results
    }

    async fn execute_task(&self, task: &str) -> Result<String> {
        info!("Executing cron task: {}", task);
        Ok(format!("Task completed: {}", task))
    }
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new()
    }
}
