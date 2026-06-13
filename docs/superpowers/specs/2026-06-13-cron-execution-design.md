# Cron Execution — Design (cluster batch 3 of 4)

Date: 2026-06-13
Status: Approved (cluster continuation; user: "continue")

## Context

`CronScheduler` (`src/cron/scheduler.rs`) has job structs, add/remove/list, and a
`tick()` that detects due jobs — but `execute_task` is a stub (`"Task completed: {task}"`),
there's no persistence, and **nothing calls `tick()`**. Per the exploration, cron should
**reuse the proactive-loop infrastructure** I already shipped (`src/gateway/proactive.rs`:
60s tick, live config reload, `post_everywhere` delivery) rather than spin up a parallel
scheduler loop.

## Architecture

### `src/cron/scheduler.rs` — make it a real, persisted store
- Persist to `<data_dir>/cron.json` (Vec<CronJob>): `load(data_dir) -> Self`,
  `save(data_dir) -> Result<()>` (atomic tempfile). `add_job` gains a `delivery_target`
  and returns the id; `remove_job` matches id or id-prefix; `list_jobs`, `enable_job` stay.
- Pure helper `schedule_due(schedule, last_run, now) -> bool` supporting `"every <N>m"`,
  `"every <N>h"`, `"every <N>d"` (unknown formats → never due; logged). Unit-tested.
- `take_due(&mut self, now) -> Vec<CronJob>` — returns enabled jobs whose schedule is due,
  marking `last_run = now`, `run_count += 1` on each (caller persists + executes). Replaces
  the `tick()`/`execute_task` stub (both removed — execution moves to the caller, which has
  the config/agent/delivery the scheduler shouldn't own).

### Execution + delivery (`src/gateway/proactive.rs`)
- `run_cron_step(cfg)`: `CronScheduler::load`, `take_due(now)`; for each due job run its
  `task` prompt through a fresh `Agent` (non-operator → no hooks for unattended runs; tools
  per `config.tools.enabled`; permission mode = `permissions.gateway_mode`), then deliver
  the result via `post_everywhere(cfg, "⏰ {name}\n\n{result}")`. Persist the scheduler
  after marking runs. Called in the 60s loop alongside `run_brief_step`/`run_watcher_step`.
- Best-effort: a job whose agent errors delivers the error text; no job can crash the loop.

### CLI (`src/main.rs`)
- `openassistant cron --action list|add|remove|run`:
  - `add --name <n> --schedule "every 60m" --task "<prompt>"` (optional `--deliver <t>`)
  - `remove --id <id>` (prefix ok), `list`, `run` (run all currently-due jobs now and print
    each result — lets the operator test a job without the gateway).

## Why reuse the proactive loop (not a parallel scheduler)

The proactive loop already ticks every 60s, reloads config live, and owns
`post_everywhere`. A second scheduler task would double-tick and duplicate delivery. Cron
becomes a third responsibility of the one loop — consistent with brief + watchers.

## Non-goals

- Real crontab expressions (`0 8 * * *`) — `"every <N>{m,h,d}"` only for v1 (no cron crate).
- Per-job delivery routing beyond `post_everywhere` (Discord home + configured Telegram);
  `delivery_target` is stored for future routing but v1 always uses `post_everywhere`.

## Error handling

Missing/corrupt `cron.json` → empty scheduler (logged). Agent/delivery failures are logged
and turned into delivered error text; the loop continues.

## Testing

- `schedule_due`: every Nm/Nh/Nd boundaries (just-before / at / after), never-run (last=None)
  → due, unknown format → not due.
- `take_due`: returns only due+enabled jobs, marks last_run/run_count; second immediate call
  returns nothing.
- `load`/`save` round-trip in a temp dir; `remove_job` by prefix.
- `cargo test --workspace` green; clippy no new warnings.
