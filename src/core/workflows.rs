// src/core/workflows.rs
//! Dynamic workflow orchestration — parallel fan-out/fan-in for large tasks
//! Like Claude Code's dynamic workflows (10s-100s of parallel agents)

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, debug};

use crate::config::Config;

// ─── Workflow Definition ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub description: String,
    pub agent_type: Option<String>,
    pub tools: Option<Vec<String>>,
    /// Steps this step depends on (must complete first)
    pub depends_on: Option<Vec<String>>,
    /// Whether this step can run in parallel with siblings
    #[serde(default)]
    pub parallel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
}

// ─── Workflow State ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct WorkflowRun {
    pub id: String,
    pub name: String,
    pub status: WorkflowStatus,
    pub steps: HashMap<String, StepResult>,
    pub total_steps: usize,
    pub completed_steps: usize,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_id: String,
    pub status: WorkflowStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

// ─── Workflow Engine ──────────────────────────────────────────────────

/// Lightweight persisted run summary (read back from `workflows.db`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunRow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub total_steps: usize,
    pub completed_steps: usize,
    pub start_time: String,
    pub end_time: Option<String>,
}

/// SQLite-backed persistence for workflow runs + step results
/// (`~/.openassistant/workflows.db`). Kept separate from `memory.db` so the FTS
/// store is not disturbed. The `Connection` is `!Send`; it stays on the engine
/// task and is never moved into a spawned step.
#[derive(Debug)]
pub struct WorkflowStore {
    conn: Connection,
}

impl WorkflowStore {
    pub fn open_default() -> Result<Self> {
        let data_dir = crate::config::data_dir_default();
        std::fs::create_dir_all(&data_dir).ok();
        Self::open(&format!("{}/workflows.db", data_dir))
    }

    pub fn open(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workflow_runs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                status TEXT NOT NULL,
                total_steps INTEGER NOT NULL,
                completed_steps INTEGER NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT
            );
            CREATE TABLE IF NOT EXISTS workflow_step_results (
                run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
                step_id TEXT NOT NULL,
                status TEXT NOT NULL,
                output TEXT,
                error TEXT,
                start_time TEXT NOT NULL,
                end_time TEXT,
                PRIMARY KEY (run_id, step_id)
            );",
        )?;
        Ok(())
    }

    pub fn upsert_run(&self, run: &WorkflowRun) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workflow_runs (id, name, status, total_steps, completed_steps, start_time, end_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                completed_steps = excluded.completed_steps,
                end_time = excluded.end_time",
            params![
                run.id,
                run.name,
                format!("{:?}", run.status),
                run.total_steps as i64,
                run.completed_steps as i64,
                run.start_time.to_rfc3339(),
                run.end_time.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_step(&self, run_id: &str, step: &StepResult) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workflow_step_results (run_id, step_id, status, output, error, start_time, end_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(run_id, step_id) DO UPDATE SET
                status = excluded.status,
                output = excluded.output,
                error = excluded.error,
                end_time = excluded.end_time",
            params![
                run_id,
                step.step_id,
                format!("{:?}", step.status),
                step.output,
                step.error,
                step.start_time.to_rfc3339(),
                step.end_time.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn list_runs(&self) -> Result<Vec<WorkflowRunRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, status, total_steps, completed_steps, start_time, end_time
             FROM workflow_runs ORDER BY start_time DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(WorkflowRunRow {
                id: row.get(0)?,
                name: row.get(1)?,
                status: row.get(2)?,
                total_steps: row.get::<_, i64>(3)? as usize,
                completed_steps: row.get::<_, i64>(4)? as usize,
                start_time: row.get(5)?,
                end_time: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_run(&self, run_id: &str) -> Result<Option<WorkflowRunRow>> {
        let res = self.conn.query_row(
            "SELECT id, name, status, total_steps, completed_steps, start_time, end_time
             FROM workflow_runs WHERE id = ?1",
            params![run_id],
            |row| {
                Ok(WorkflowRunRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    status: row.get(2)?,
                    total_steps: row.get::<_, i64>(3)? as usize,
                    completed_steps: row.get::<_, i64>(4)? as usize,
                    start_time: row.get(5)?,
                    end_time: row.get(6)?,
                })
            },
        );
        match res {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Debug)]
pub struct WorkflowEngine {
    workflows: HashMap<String, WorkflowDef>,
    active_runs: HashMap<String, Arc<Mutex<WorkflowRun>>>,
    /// When set, steps execute real LLM calls; when `None` (the `Default`/`new`
    /// path) the engine retains its original simulate-only behavior so existing
    /// callers do not break.
    config: Option<Arc<Config>>,
    db: Option<WorkflowStore>,
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkflowEngine {
    /// Stub engine: no config, no persistence, simulate-only step execution.
    pub fn new() -> Self {
        Self {
            workflows: HashMap::new(),
            active_runs: HashMap::new(),
            config: None,
            db: None,
        }
    }

    /// Real engine: executes steps via the LLM (resolved for the `text`
    /// modality) and persists runs to `workflows.db`.
    pub fn new_with_config(config: Arc<Config>, db_path: &str) -> Result<Self> {
        Ok(Self {
            workflows: HashMap::new(),
            active_runs: HashMap::new(),
            config: Some(config),
            db: Some(WorkflowStore::open(db_path)?),
        })
    }

    pub fn register_workflow(&mut self, def: WorkflowDef) {
        info!("Registered workflow: {} ({} steps)", def.name, def.steps.len());
        self.workflows.insert(def.name.clone(), def);
    }

    /// Execute a workflow with parallel fan-out/fan-in respecting dependencies.
    /// `input` (if any) is provided as context to root steps (those with no
    /// dependencies). Each step is one LLM call when the engine has a config.
    pub async fn execute(&mut self, workflow_name: &str, input: Option<&str>) -> Result<String> {
        let workflow = self.workflows.get(workflow_name)
            .ok_or_else(|| anyhow::anyhow!("Workflow not found: {}", workflow_name))?
            .clone();

        let run_id = format!("wf_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        info!("Starting workflow run: {} ({})", run_id, workflow_name);

        // Resolve the LLM target once (text modality). `None` => simulate path.
        let llm = self.config.as_ref().map(|cfg| {
            let (b, k, m) = crate::config::resolve_provider(cfg, "text");
            (b.to_string(), k.to_string(), m.to_string())
        });
        let client = reqwest::Client::new();

        let mut run = WorkflowRun {
            id: run_id.clone(),
            name: workflow_name.to_string(),
            status: WorkflowStatus::Running,
            steps: HashMap::new(),
            total_steps: workflow.steps.len(),
            completed_steps: 0,
            start_time: chrono::Utc::now(),
            end_time: None,
        };
        if let Some(db) = &self.db {
            let _ = db.upsert_run(&run);
        }

        let mut completed: HashMap<String, StepResult> = HashMap::new();
        let mut remaining: Vec<_> = workflow.steps.clone();

        while !remaining.is_empty() {
            // Ready = every dependency is present AND succeeded.
            let ready: Vec<_> = remaining
                .iter()
                .filter(|step| {
                    step.depends_on.as_ref().map_or(true, |deps| {
                        deps.iter().all(|d| {
                            completed.get(d).map_or(false, |r| r.status == WorkflowStatus::Completed)
                        })
                    })
                })
                .cloned()
                .collect();

            if ready.is_empty() {
                // Distinguish a failed dependency from a genuine deadlock.
                let blocked: Vec<_> = remaining
                    .iter()
                    .filter(|step| {
                        step.depends_on.as_ref().map_or(false, |deps| {
                            deps.iter().any(|d| {
                                completed.get(d).map_or(false, |r| r.status == WorkflowStatus::Failed)
                            })
                        })
                    })
                    .cloned()
                    .collect();

                if blocked.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Workflow deadlock — no ready steps and no failed dependencies (cyclic depends_on?)"
                    ));
                }

                for step in &blocked {
                    let dep = step
                        .depends_on
                        .as_ref()
                        .and_then(|deps| {
                            deps.iter().find(|d| {
                                completed.get(*d).map_or(false, |r| r.status == WorkflowStatus::Failed)
                            })
                        })
                        .cloned()
                        .unwrap_or_default();
                    let sr = StepResult {
                        step_id: step.id.clone(),
                        status: WorkflowStatus::Failed,
                        output: None,
                        error: Some(format!("skipped: dependency '{}' failed", dep)),
                        start_time: chrono::Utc::now(),
                        end_time: Some(chrono::Utc::now()),
                    };
                    if let Some(db) = &self.db {
                        let _ = db.upsert_step(&run_id, &sr);
                    }
                    completed.insert(step.id.clone(), sr);
                }
                remaining.retain(|s| !completed.contains_key(&s.id));
                continue;
            }

            // Run all ready steps in parallel. The spawned futures capture only
            // owned, Send data (a cloned Client + owned strings) — never the
            // engine's !Send SQLite connection.
            let mut handles = Vec::new();
            for step in &ready {
                let step_clone = step.clone();
                let dep_context = step_clone
                    .depends_on
                    .as_ref()
                    .map(|deps| {
                        deps.iter()
                            .filter_map(|d| {
                                completed.get(d).and_then(|r| r.output.as_ref()).map(|o| {
                                    format!("### Output of step '{}':\n{}", d, o)
                                })
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    })
                    .unwrap_or_default();
                let is_root = step_clone.depends_on.as_ref().map_or(true, |d| d.is_empty());
                let input_ctx = if is_root { input.map(|s| s.to_string()) } else { None };
                let llm_clone = llm.clone();
                let client_clone = client.clone();

                let handle = tokio::spawn(async move {
                    let start = chrono::Utc::now();
                    debug!("Executing workflow step: {}", step_clone.id);

                    match &llm_clone {
                        Some((base, key, model)) => {
                            let mut user = String::new();
                            if let Some(inp) = &input_ctx {
                                if !inp.is_empty() {
                                    user.push_str("Input:\n");
                                    user.push_str(inp);
                                    user.push_str("\n\n");
                                }
                            }
                            if !dep_context.is_empty() {
                                user.push_str("Context from previous steps:\n");
                                user.push_str(&dep_context);
                                user.push_str("\n\n");
                            }
                            user.push_str(&format!("Task: {}", step_clone.description));

                            let messages = vec![
                                serde_json::json!({
                                    "role": "system",
                                    "content": format!(
                                        "You are executing step '{}' of a multi-step workflow. \
                                         Produce a focused, useful result for THIS step only.",
                                        step_clone.id
                                    )
                                }),
                                serde_json::json!({ "role": "user", "content": user }),
                            ];

                            match super::agent::call_llm_raw(&client_clone, base, key, model, &messages).await {
                                Ok(out) => StepResult {
                                    step_id: step_clone.id.clone(),
                                    status: WorkflowStatus::Completed,
                                    output: Some(out),
                                    error: None,
                                    start_time: start,
                                    end_time: Some(chrono::Utc::now()),
                                },
                                Err(e) => StepResult {
                                    step_id: step_clone.id.clone(),
                                    status: WorkflowStatus::Failed,
                                    output: None,
                                    error: Some(e.to_string()),
                                    start_time: start,
                                    end_time: Some(chrono::Utc::now()),
                                },
                            }
                        }
                        None => StepResult {
                            step_id: step_clone.id.clone(),
                            status: WorkflowStatus::Completed,
                            output: Some(format!(
                                "[simulated] Step '{}': {}",
                                step_clone.id, step_clone.description
                            )),
                            error: None,
                            start_time: start,
                            end_time: Some(chrono::Utc::now()),
                        },
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                match handle.await {
                    Ok(result) => {
                        if let Some(db) = &self.db {
                            let _ = db.upsert_step(&run_id, &result);
                        }
                        completed.insert(result.step_id.clone(), result);
                    }
                    Err(e) => {
                        tracing::warn!("Workflow step task panicked: {}", e);
                    }
                }
            }

            remaining.retain(|s| !completed.contains_key(&s.id));
        }

        let succeeded = completed.values().filter(|r| r.status == WorkflowStatus::Completed).count();
        let any_failed = completed.values().any(|r| r.status == WorkflowStatus::Failed);

        run.status = if any_failed { WorkflowStatus::Failed } else { WorkflowStatus::Completed };
        run.completed_steps = succeeded;
        run.end_time = Some(chrono::Utc::now());
        run.steps = completed.clone();

        if let Some(db) = &self.db {
            let _ = db.upsert_run(&run);
        }
        let run_arc = Arc::new(Mutex::new(run.clone()));
        self.active_runs.insert(run_id.clone(), run_arc);

        info!(
            "Workflow run {} finished: {}/{} steps ({:?})",
            run_id, succeeded, run.total_steps, run.status
        );

        // Build a readable transcript in declared step order.
        let mut details = String::new();
        for step in &workflow.steps {
            if let Some(r) = completed.get(&step.id) {
                details.push_str(&format!("\n── {} [{:?}] ──\n", step.id, r.status));
                if let Some(o) = &r.output {
                    details.push_str(o.trim());
                    details.push('\n');
                }
                if let Some(e) = &r.error {
                    details.push_str(&format!("error: {}\n", e));
                }
            }
        }

        Ok(format!(
            "Workflow '{}' [{}] {:?}: {}/{} steps in {:?}\n{}",
            workflow_name,
            run_id,
            run.status,
            succeeded,
            run.total_steps,
            run.end_time.unwrap() - run.start_time,
            details
        ))
    }

    pub fn get_run(&self, run_id: &str) -> Option<Arc<Mutex<WorkflowRun>>> {
        self.active_runs.get(run_id).cloned()
    }

    /// Persisted run history (empty for the stub engine without a DB).
    pub fn list_runs(&self) -> Result<Vec<WorkflowRunRow>> {
        match &self.db {
            Some(db) => db.list_runs(),
            None => Ok(Vec::new()),
        }
    }

    /// Status of a persisted run by id (e.g. "Completed", "Failed").
    pub fn get_run_status(&self, run_id: &str) -> Result<Option<String>> {
        match &self.db {
            Some(db) => Ok(db.get_run(run_id)?.map(|r| r.status)),
            None => Ok(self
                .active_runs
                .get(run_id)
                .map(|_| "Running".to_string())),
        }
    }

    pub fn list_workflows(&self) -> Vec<&WorkflowDef> {
        self.workflows.values().collect()
    }
}

// ─── Built-in Workflows ───────────────────────────────────────────────

pub fn built_in_workflows() -> Vec<WorkflowDef> {
    // The previous `code-review` built-in declared tool-using steps (glob/grep/
    // read), but workflow steps do not have tool dispatch — it would have
    // produced confident hallucinations. This tool-free `analyze → critique →
    // summarize` chain operates purely on the `--input` text and actually runs.
    vec![WorkflowDef {
        name: "analyze".to_string(),
        description: "Analyze → critique → summarize the provided input (tool-free).".to_string(),
        steps: vec![
            WorkflowStep {
                id: "analyze".to_string(),
                description: "Analyze the input: identify the key points, structure, and intent.".to_string(),
                agent_type: None,
                tools: None,
                depends_on: None,
                parallel: false,
            },
            WorkflowStep {
                id: "critique".to_string(),
                description: "Critique the analysis: surface gaps, risks, and counterpoints.".to_string(),
                agent_type: None,
                tools: None,
                depends_on: Some(vec!["analyze".to_string()]),
                parallel: false,
            },
            WorkflowStep {
                id: "summarize".to_string(),
                description: "Produce a concise, balanced summary combining the analysis and the critique.".to_string(),
                agent_type: None,
                tools: None,
                depends_on: Some(vec!["analyze".to_string(), "critique".to_string()]),
                parallel: false,
            },
        ],
    }]
}
