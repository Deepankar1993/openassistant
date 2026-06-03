# Research Notes

Produced by a parallel agent team (6 researchers on Sonnet -> 3-persona panel on Sonnet -> lead synthesis on Opus), run 2026-06-03. All claims were verified against source. The synthesis below is the decision of record; the per-feature briefs and the panel review follow as supporting material.


---

# Lead Synthesis (decision of record)

Now I have verified all disputed facts. Key confirmations: `config::set` has NO `model.api_base`, NO `gateway.discord_allowed_users`, NO `gateway.dm_policy` (panel was right). `ModelConfig` has no struct-level `#[serde(default)]` (backward-compat risk confirmed). `FullContext::new()` exists (takes no args), `Persona::load_or_default(data_dir)` exists. `Milestone` callers are all internal (`create_milestone`, `complete_milestone`, `goal_progress`, `format_board`). I have enough to write the decision document.

# openAssistant — Six-Feature Decision Document

Lead architect synthesis of research + three-panel review (Industry Veteran, Senior Dev, Devil's Advocate). All claims verified against source on 2026-06-03. This drives the OpenSpec change (proposal.md + design.md + spec deltas + tasks.md).

---

## 1. Priority Verdict

The panel converged. The forced ranking, reconciled:

| Priority | Feature | Verdict rationale |
|---|---|---|
| **P0** | **update** | ~80 lines in `main.rs` + 2 helpers; `SelfUpdater` already works; zero new deps; immediate value. |
| **P0** | **checkpoint** | Foundational correctness — every restart currently destroys all state. The doc comment already lies ("Uses SQLite"); the `Vec` must become real SQLite. Zero new deps. |
| **P1** | **workflow** | DAG loop is already correct; only the execution closure is fake. Unblocks `call_llm_raw`, which `multimodel` and `subgoals` both reuse. |
| **P1** | **multimodel** | Config schema + `resolve_provider` + `call_llm` wiring only. Pure backward-compatible. Vision-API/image-gen are explicitly cut to P2. |
| **P2** | **subgoals** | Persisting an ephemeral stub that emits placeholder text is building a database for fake data. **Blocked on** `call_llm_raw` (P1 workflow) so `GoalDeliberator` produces real output worth persisting. |
| **P2** | **discord** | Only feature adding a major new dependency tree (serenity, ~15 crates). Convenience channel, not core. Manual portal config can't be CI-tested. |

**Hard sequencing constraint:** `call_llm_raw` (extracted in P1-workflow) is a shared dependency for P1-multimodel and P2-subgoals. It is the linchpin — build it first inside the workflow change.

---

## 2. Per-Feature Design Decisions

### 2.1 `update` (P0)

**Chosen approach:** Git-based source update only. Do **not** call `check_update()` (crates.io path is permanently dead — crate unpublished, returns `Ok(None)` on 404). Replace its body with `anyhow::bail!("not published to crates.io; use check_pending()")` so it stops being a silent lie (DA finding).

**Resolved decisions:**
- **Workspace dir source of truth:** `OPENASSISTANT_SRC` env var is **primary, not an escape hatch** (DA/Senior Dev correct: installed binary at `%USERPROFILE%\.cargo\bin\` makes the exe-parent walk fail for all non-dev Windows users). Order: (1) `OPENASSISTANT_SRC`, (2) exe-parent walk for `Cargo.toml` with `name = "open-assistant"`, (3) error naming the env var. Document at the top of the arm: "`update` only works for source checkouts."
- **`--check` exit code:** Exit 0 always for MVP (Veteran). Scriptable exit codes deferred — not user value yet.
- **Confirmation:** Implement `--yes` / stdin prompt in the CLI arm. The `SelfUpdater.permission` field (`!= Deny` check) is effectively dead in this wiring; gate on the CLI prompt instead and pass `Permission::Allow` only post-confirmation, or document the field as unused.

**Must-fix bugs surfaced by panel (do in this change):**
- `run_cmd` line 167: `.await.unwrap_or(Ok(Output{ status: ExitStatus::default(), ...}))` silently converts a panicked `cargo build` into fake success. Replace with `.await??` (propagate `JoinError` + `io::Error`). It compiles on current stable but the silent-swallow is a correctness hole.
- Add a dirty-tree gate (`git status --porcelain`) before `git pull --rebase`, else rebase aborts with an opaque error.
- Output message must say: build wrote a new binary to `target/release/`; if installed via `cargo install`, run `cargo install --path .` to update PATH; the *running* process keeps the old `CARGO_PKG_VERSION` until restart.
- If `git`/`cargo` not on PATH (GUI/service launch), surface "cargo not found — ensure Rust is on PATH" rather than a raw `io::Error`.

**Files/functions:**
- `src/main.rs` — change `Update` variant to `Update { check: bool, yes: bool }`; replace the 4-line placeholder arm (lines ~118–121) with the flow (detect dir → print version → `check_pending()` → if empty "up to date" exit 0 → if `--check` print & exit → else prompt unless `--yes` → dirty check → call `update_from_source()`). Import `open_assistant::core::self_update::SelfUpdater`.
- `src/core/self_update.rs` — add `pub async fn check_pending(&self) -> Result<Vec<String>>` (`git fetch` then `git log HEAD..origin/HEAD --oneline`) and `pub async fn is_dirty(&self) -> Result<bool>` (`git status --porcelain`). Fix `run_cmd`. Neuter `check_update()`.
- `src/core/agent.rs` (~line 554) — one-string edit on the `self_update` tool response to point at `openassistant update --check`.

**Crates:** none.

---

### 2.2 `workflow` (P1)

**Chosen approach:** **Option B** — extract a shared raw LLM call, one LLM call per step, no `Agent::process()`, no tool dispatch, no `MemoryWorkspace` writes (avoids the multi-week concurrency race the panel unanimously flagged). Persist runs to a **dedicated `workflows.db`** (Veteran + DA: keeps the FTS memory store clean; lets WAL be set independently).

**Resolved decisions:**
- **`call_llm_raw` location & visibility:** `pub(crate) async fn call_llm_raw(client: &reqwest::Client, model_config: &ModelConfig, messages: &[serde_json::Value]) -> Result<String>` as a free function in `src/core/agent.rs`. `Agent::call_llm` becomes a thin wrapper. **Pass a shared `reqwest::Client`** (Senior Dev: current code builds a new `Client` per call at agent.rs:181 — fix it here; construct once in `execute()`, clone the `Arc` into each spawn). `ModelConfig` is all-`String` so it `Clone`s trivially → `Arc<ModelConfig>` into closures.
- **Keep the `Default` impl** for `WorkflowEngine` (DA: removing it breaks `WorkflowEngine::default()`/`new()` callers and the stub CLI path). Add `WorkflowEngine::new_with_config(config: Arc<Config>, db_path: &str) -> Result<Self>` as an additional constructor. `Default` keeps the stub behavior; real execution requires `new_with_config`.
- **`WorkflowStatus` → TEXT:** use `format!("{:?}", status)` for INSERT and a `match` on read — not `serde_json` (would yield quoted `"Completed"`). `WorkflowRun`/`StepResult` derive only `Debug, Clone`; use explicit `row.get::<_, String>(n)` column extraction, not serde.
- **Failed-dependency handling (DA):** when a step errors, record a `Failed` `StepResult` into `completed` so dependents become detectably unsatisfiable, and emit "step X skipped: dependency Y failed" instead of the misleading "Workflow deadlock" at line 134.
- **`code-review` built-in is meaningless without tools (all three panels):** replace `built_in_workflows()` `code-review` with a tool-free workflow that actually works (e.g. `analyze` → `critique` → `summarize` operating on text the user provides), OR tag steps `[DEMO — needs tool dispatch]` in output. Decision: **replace it** for MVP honesty.

**Files/functions:**
- `src/core/agent.rs` — extract `call_llm_raw` (from lines 178–223); `Agent::call_llm` delegates; add `modality` plumbing is **not** done here (that's multimodel — but coordinate the signature so multimodel slots in cleanly).
- `src/core/workflows.rs` — new `WorkflowStore` (rusqlite, `PRAGMA journal_mode=WAL`, `PRAGMA foreign_keys=ON`) with `open`, `upsert_run`, `upsert_step`, `list_runs`, `get_run`; add `config: Arc<Config>` + `db: WorkflowStore` fields; `new_with_config`; rewrite the spawn closure (lines 139–162) to call `call_llm_raw` with prior-step outputs injected; write `StepResult`s to SQLite in the join loop (after line 169, on the engine thread — `Connection` is `!Send`, must stay off the spawn); fix `list_runs()` to query SQLite; add `get_run_status(run_id)`; fix the deadlock-vs-failed-dep message.
- `src/main.rs` (lines ~271–279) — `config::load().await?`, wrap in `Arc`, call `WorkflowEngine::new_with_config`.

**Crates:** none (`rusqlite 0.32` bundled already present).

---

### 2.3 `checkpoint` (P0)

**Chosen approach:** Replace `Vec<Checkpoint>` with `rusqlite::Connection`. **Use a dedicated `checkpoints.db`**, not `memory.db` (DA: file snapshots are multi-MB text → page-cache pressure on the FTS store; `MemoryStore::open()` doesn't enable `foreign_keys` so reusing it verbatim is unsafe for cascade). This overrides the original research lean toward `memory.db`; the panel's counter-argument is stronger.

**Resolved decisions:**
- **Lazy vs eager file content:** `list_checkpoints` returns lightweight metadata (id, session_id, timestamp, description, file_count) — confirmed **no external callers** (only `format_checkpoints`, `latest_checkpoint`, and `main.rs`; `agent.rs` does not touch `CheckpointStore`). Keep a separate `load_checkpoint(id)` that fetches file rows. API split is safe.
- **`PRAGMA foreign_keys = ON` on every `open()`** (per-connection, SQLite default off) for `ON DELETE CASCADE` to work.
- **Keep `CheckpointStore` sync** (`fn open()`, not `async`) — `Connection` is `!Send`; the `main.rs` checkpoint arm is sequential and never `.await`s while holding it. Do not make any method `async`.
- **Restore safety (DA data-loss vector):** before `std::fs::write` overwrite, compare current-file SHA-256 against the snapshot hash; if changed, warn and skip unless `--force`. Stops silent clobbering of unsaved work.
- **UTF-8 only:** `read_to_string` already silently drops binary files; document this — `content TEXT` has the same limitation.
- **Arg overloading fix:** split `--id` (checkpoint id) from `--session` (session id). `--session` defaults to a hash of the absolute workspace path (stable, not the raw path — survives nothing if renamed, but neither does the current scheme; document it). Existing `--action list --id <session>` scripts break — note in spec.
- **`delete` action:** deferred to P2 (Veteran). MVP = `list`, `create`, `restore`.
- **Rewrite the two existing tests** to pass a temp DB path (they currently test the `Vec` path and would give false confidence).

**Files/functions:**
- `src/core/checkpoint.rs` — replace struct internals with `conn: Connection`; add `open(db_path)` / `open_default()` (mirror `MemoryStore`, plus the pragmas); reimplement `create_checkpoint` (transaction: INSERT run + batch INSERT files + prune), `restore_checkpoint` (with hash check), `list_checkpoints` (metadata), new `load_checkpoint`, `latest_checkpoint`, `format_checkpoints`, `prune_old_checkpoints` (DELETE with `LIMIT` subquery, cascade handles files). Rewrite tests.
- `src/main.rs` — `Checkpoint { action, id, session, workspace, files, description }`; wire `list`/`create`/`restore`; `CheckpointStore::open_default()?`. Note: `open_default()` is now fallible — a DB error `?`-propagates and exits the process; acceptable for CLI MVP but the arm should print a friendly message.

**Crates:** none (`rusqlite 0.32`, `sha2 0.10` present).

---

### 2.4 `multimodel` (P1)

**Chosen approach:** Config schema + `resolve_provider` + `call_llm` wiring **only**. Vision-API branch, image-gen, video stubs, and `config::set` expansion are **all cut to P2** (Veteran: bundling them is scope creep).

**Resolved decisions:**
- **Backward compat (Senior Dev/DA — real risk):** `ModelConfig` has **no struct-level `#[serde(default)]`** (verified). New `ProviderEntry`/`ModalityRoute`/`RoutingConfig` get `#[serde(default)]` at struct level **and** field defaults. Add `providers: Vec<ProviderEntry>` and `routing: RoutingConfig` to `Config` with `#[serde(default)]`. Add a YAML round-trip test that loads a legacy `[model]`-only config to prove no regression.
- **Behavioral change (DA):** once `call_llm` resolves the model via `resolve_provider`, `Agent::new("some-model")` no longer controls the model when a `routing.text` entry exists. **Resolution:** if `routing.text.provider` is empty, fall straight through to `config.model.*` AND keep using `self.model` as the body's `model` field — i.e. routing is strictly opt-in; empty routing = today's exact behavior. This preserves the desktop `Agent::new(cfg.model.model)` semantics until a user explicitly populates `routing`.
- **`config::set` allowlist (Senior Dev security framing):** do **not** broaden it for `routing.*`/`providers.*`/`model.api_base` in MVP. Document that multi-provider config is set by editing `config.yaml` (the allowlist is security-by-design against URL injection).
- **Vision dispatch (Decision A):** **Option 1** for MVP (vision tool resolves its own provider), BUT route both through the shared `call_llm_raw` from the workflow change to avoid the duplicate-HTTP-path hazard the DA flagged. This is why workflow lands first.

**Files/functions:**
- `src/config/mod.rs` — add `ProviderEntry`, `ModalityRoute`, `RoutingConfig` (all `#[serde(default)]` + `Default`); extend `Config`; add `pub fn resolve_provider<'a>(config: &'a Config, modality: &str) -> (&'a str, &'a str, &'a str)`.
- `src/core/agent.rs` — `call_llm`/`call_llm_raw` use `resolve_provider` instead of `config.model.*` directly; thread `modality: &str` (callers pass `"text"`). One internal call site at agent.rs:94.

**Crates:** none.

---

### 2.5 `subgoals` (P2 — blocked on P1 `call_llm_raw`)

**Chosen approach:** Add a `Subgoal` layer; **JSON persistence** at `~/.openassistant/goals.json` for MVP, SQLite as the next PR. **But sequence after `GoalDeliberator` makes real LLM calls** (using `call_llm_raw`) — persisting placeholder stub text is worthless (Veteran + DA).

**Resolved decisions:**
- **Drop `Milestone`, clean path** (Veteran verdict; verified all callers internal: `create_milestone`, `complete_milestone`, `goal_progress` line 539, `format_board` line 596 — **add `format_board` to the touch list**, the research omitted it).
- **Atomic JSON write (DA):** use the `tempfile` crate's `NamedTempFile` + `persist()`, not `std::fs::write` + rename (which isn't atomic and fails cross-volume on Windows `ERROR_NOT_SAME_DEVICE`). `tempfile` is already a dev-dep; promote to a regular dep or add manual temp-in-same-dir + rename.
- **Concurrency (DA):** two simultaneous `goals` CLI invocations race on `goals.json` (last-writer-wins). Acknowledge as a known JSON limitation; SQLite (next PR) fixes it. Acceptable for MVP single-user.
- **Module wiring (Senior Dev):** new `goal_store.rs` is registered via `pub mod goal_store;` in `src/core/mod.rs`, not `src/lib.rs`.
- **Tool touch-points:** new `goal_*` tools only touch `handle_tool_calls` + `default_tools()` in `agent.rs` — **not** `ToolRegistry::execute` (agent-internal tools like `goal_deliberate`/`todo_write` are caught in `handle_tool_calls` before `ToolRegistry`). Drop `src/tools/mod.rs` from the touch list (research self-contradicted).

**Files/functions:**
- `src/core/goal_system.rs` — add `Subgoal`; add `subgoals: HashMap<String,Subgoal>` + `#[derive(Serialize, Deserialize)]` to `TaskBoard`; delete `Milestone` and migrate `create_milestone`/`complete_milestone`/`goal_progress`/`format_board` to subgoals; add `Goal::progress(&board)` rollup.
- `src/core/goal_store.rs` (new) — `GoalStore { path, board }` with atomic load/save + CRUD delegates.
- `src/core/mod.rs` — `pub mod goal_store;`.
- `src/core/agent.rs` — make `handle_goal_deliberate` call `call_llm_raw` per role then persist via `GoalStore`; add `goal_create`/`goal_subgoal`/`goal_list`/`goal_task` handlers + `default_tools()` entries.
- `src/main.rs` — `Goals { action, goal, subgoal, title, description }` command.

**Crates:** `tempfile = "3"` promoted from dev-dep to dep (for atomic write). No SQLite in MVP.

---

### 2.6 `discord` (P2)

**Chosen approach:** `serenity` with minimal features, no cache, per-user `SessionState`. As designed in research, with panel corrections.

**Resolved decisions:**
- **Crate version:** `serenity = { version = "0.12", default-features = false, features = ["client","gateway","model","http","builder","rustls_backend"] }`. Specify `"0.12"` (semver range), never the patch. **Verify serenity 0.12's internal reqwest resolves to 0.12 and its TLS feature is rustls** before claiming dedup (Senior Dev: if serenity pulls native-tls, two TLS stacks ship). Confirm `rust-version` ≥ serenity MSRV (~1.74); the project pins none.
- **Cache contradiction (DA):** research says "log a warning if bot is in any guilds at startup" but excludes `cache`, which is what enumerates guilds. **Resolution:** drop the guild-enumeration warning; instead log a one-time startup line "MESSAGE_CONTENT intent must be enabled in the Developer Portal or message text will be empty." Honest, cache-free.
- **Async Mutex across await (Senior Dev/DA):** use `tokio::sync::Mutex` for `Arc<Mutex<HashMap<u64, SessionState>>>`. Pattern: lock → get-or-insert → **clone or take the state out, drop the guard** → `agent.process().await` → re-lock to write back. Never hold the guard across `process().await` (serializes all users; with `std::sync::Mutex` it would not compile in async). Document explicitly.
- **`FullContext` construction (verified):** `FullContext::new()` exists (no args, uses `UserModel::default()`); `Persona::load_or_default(data_dir)` exists. Build per-user `FullContext::new()` then optionally swap in the loaded persona. No deferred verification needed.
- **Session growth → P0-within-this-feature, not P1 (DA):** Discord is long-running (unlike CLI). Bound `Session::messages` at construction or trim after each `process()`. `call_llm` already trims to 30 for the HTTP body, but the in-memory `Session` grows unbounded.
- **Per-user keying** (`UserId`) for MVP; `(UserId, ChannelId)` via a future `gateway.session_scope` config.
- **Error propagation (Senior Dev):** Discord is spawned fire-and-forget while `webchat::start()` blocks the main task. A Discord panic is silently lost. Use a `JoinSet`/`tokio::select!` or at minimum log on the spawned task's error.

**Files/functions:**
- `Cargo.toml` — add serenity.
- `src/gateway/discord.rs` — full rewrite: `Handler { agent, sessions, allowed_users, dm_policy }`, `EventHandler::message`, `start(token, gateway_cfg, model_cfg)`, keep `is_allowed`.
- `src/gateway/mod.rs` (~line 19) — uncomment, change signature, `tokio::spawn` with error logging.
- `src/config/mod.rs` (`set`) — add `gateway.discord_allowed_users` (comma-split) and `gateway.dm_policy` (both confirmed missing from the allowlist).

**Crates:** `serenity 0.12` (default-features off; ~15 transitive).

---

## 3. Config Schema (concrete)

### Multi-model providers + routing (backward compatible)

```yaml
# Existing block — UNCHANGED. Empty routing => this is used verbatim.
model:
  provider: openrouter
  model: openrouter/owl-alpha
  api_key: ""
  api_base: https://openrouter.ai/api/v1

# NEW — both default to empty; legacy configs load with zero behavior change.
providers:
  - name: openrouter
    api_base: https://openrouter.ai/api/v1
    api_key: sk-or-...
  - name: openai
    api_base: https://api.openai.com/v1
    api_key: sk-...

routing:
  text:      { provider: openrouter, model: openrouter/owl-alpha }
  vision:    { provider: openai,     model: gpt-4o }       # P2 — parsed but not dispatched in P1
  image_gen: { provider: "",         model: "" }           # P2 stub
  video:     { provider: "",         model: "" }           # P2 stub
```

`resolve_provider(config, modality)`: if `routing.<modality>.provider` is non-empty and matches a `providers[]` entry → `(entry.api_base, entry.api_key, route.model)`; else fall through to `(config.model.api_base, config.model.api_key, config.model.model)`.

Serde rules: every new struct gets `#[serde(default)]` at struct level + a `Default` impl; `Config.providers`/`Config.routing` get field-level `#[serde(default)]`.

### Checkpoint persistence — dedicated `~/.openassistant/checkpoints.db`

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys  = ON;   -- per connection, every open()

CREATE TABLE IF NOT EXISTS checkpoints (
  id          TEXT PRIMARY KEY,
  session_id  TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  created_at  TEXT NOT NULL,            -- RFC 3339
  file_count  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_cp_session ON checkpoints(session_id, created_at);

CREATE TABLE IF NOT EXISTS checkpoint_files (
  checkpoint_id TEXT NOT NULL REFERENCES checkpoints(id) ON DELETE CASCADE,
  file_path     TEXT NOT NULL,
  file_hash     TEXT NOT NULL,          -- SHA-256 hex
  content       TEXT NOT NULL,          -- UTF-8 only; binary silently skipped
  PRIMARY KEY (checkpoint_id, file_path)
);
```

### Workflow persistence — dedicated `~/.openassistant/workflows.db`

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys  = ON;

CREATE TABLE IF NOT EXISTS workflow_runs (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, status TEXT NOT NULL,
  total_steps INTEGER NOT NULL, completed_steps INTEGER NOT NULL,
  start_time TEXT NOT NULL, end_time TEXT
);
CREATE TABLE IF NOT EXISTS workflow_step_results (
  run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
  step_id TEXT NOT NULL, status TEXT NOT NULL,
  output TEXT, error TEXT, start_time TEXT NOT NULL, end_time TEXT,
  PRIMARY KEY (run_id, step_id)
);
```

`WorkflowStatus` stored as `format!("{:?}", s)` ("Completed"), read via `match`. Columns extracted with `row.get::<_, String>(n)`, not serde.

### Goal/subgoal persistence — `~/.openassistant/goals.json` (MVP)

Single JSON document = serialized `TaskBoard { goals: HashMap<String,Goal>, tasks: HashMap<String,Task>, subgoals: HashMap<String,Subgoal> }`. `Goal.subgoals: Vec<Subgoal>` replaces `milestones`. Atomic write via `tempfile::NamedTempFile::persist()`. SQLite (three FK tables mirroring `memory.db`) is the next-PR upgrade; a `migrate_to_sqlite()` import path keeps it reversible.

---

## 4. CLI-First Ordering

User requirement: **CLI complete before desktop.** Implementation sequence:

1. **P0 `update`** (CLI-only; no desktop affordance — CAPABILITY HONESTY TABLE forbids a self-update UI without a real Tauri updater endpoint).
2. **P0 `checkpoint`** (CLI `list`/`create`/`restore`; desktop deferred).
3. **P1 `workflow`** — *build `call_llm_raw` first* (shared linchpin), then real execution + `workflows.db`. CLI `workflow <name>` + status.
4. **P1 `multimodel`** — config schema + `resolve_provider` + `call_llm` wiring (reuses `call_llm_raw`). YAML-edit only; no CLI `set` keys, no desktop.
5. **P2 `subgoals`** — only after `GoalDeliberator` calls `call_llm_raw`; `Goals` CLI + JSON store.
6. **P2 `discord`** — serenity; CLI `gateway` wiring.

**Desktop surfacing deferred (all of it), per CAPABILITY HONESTY TABLE in `src-tauri/src/lib.rs`:**
- No self-update button (needs `tauri-plugin-updater` + a published update endpoint — separate milestone).
- No workflow runner/status UI until CLI is proven and step output is real.
- No goals/subgoals panel (CLI/TUI only this cycle).
- No Discord config UI.
- Multimodel: a *read-only* providers/routing display in Settings is the only candidate, and only after the CLI path is verified — defer to a follow-up.
Checkpoint browser in desktop is a plausible first desktop follow-up once `checkpoints.db` exists, but it is **not** in this change.

---

## 5. Risks & Mitigations (top 8)

| # | Risk | Mitigation |
|---|---|---|
| 1 | **Update silently swallows a panicked `cargo build`** (`run_cmd` line 167 fake-success `Output`). | Replace `.await.unwrap_or(Ok(...))` with `.await??`; propagate `JoinError` + `io::Error`. |
| 2 | **Update workspace-dir detection fails for `cargo install` binaries on Windows** (exe-parent walk hits `%USERPROFILE%\.cargo\bin`). | Make `OPENASSISTANT_SRC` the documented primary; clear error naming it; state "source checkouts only." |
| 3 | **Built-in `code-review` workflow returns confident hallucinations** (no tools, no context). | Replace with a tool-free `analyze→critique→summarize` workflow for MVP; tool-dispatch steps deferred. |
| 4 | **`config.yaml` backward-compat break** if new structs lack defaults or legacy `[model]` lacks a field. | `#[serde(default)]` on every new struct + field; add a regression test loading a legacy-shaped config. |
| 5 | **Multimodel routing silently overrides `Agent::new(model)`** at desktop call sites. | Routing strictly opt-in: empty `routing.text` ⇒ exact current behavior (use `self.model` + `config.model.*`). |
| 6 | **Restore clobbers unsaved edits** (`fs::write` overwrite, no check). | SHA-256 compare current vs snapshot before overwrite; warn+skip unless `--force`. |
| 7 | **Discord async-Mutex held across `process().await`** serializes/deadlocks. | `tokio::sync::Mutex`; take state out + drop guard before await + re-lock to write; document. Verify serenity TLS = rustls (no dual stack) and MSRV. |
| 8 | **JSON goals store: non-atomic write + concurrent-CLI race** (truncation / lost-write on crash or parallel runs). | `tempfile::NamedTempFile::persist()` for atomicity; document single-writer assumption; SQLite next PR removes the race. |

Cross-cutting (note, not block): `config::load()` is re-read per LLM turn and per process arm — a `OnceLock<Config>` is the eventual fix but is out of scope here.

---

## 6. Task List (tasks.md)

### update (P0)
1. [P0] Fix `run_cmd` in `self_update.rs`: `.await??` propagation (remove `ExitStatus::default()` fallback).
2. [P0] Add `check_pending()` and `is_dirty()` to `SelfUpdater`; neuter `check_update()` with an explanatory `bail!`.
3. [P0] Add workspace-dir detection helper (env `OPENASSISTANT_SRC` → exe-parent walk → error) in `main.rs`.
4. [P0] Change `Commands::Update` to `{ check, yes }`; implement the 8-step flow incl. `--yes`/stdin prompt, dirty gate, post-build "restart / `cargo install --path .`" message.
5. [P0] Update the `self_update` agent-tool response string in `agent.rs`.

### checkpoint (P0)
6. [P0] Rewrite `CheckpointStore` over `rusqlite::Connection`; `open`/`open_default` for `checkpoints.db` with `journal_mode=WAL` + `foreign_keys=ON`.
7. [P0] Implement `create_checkpoint` (transaction + prune via `DELETE ... LIMIT`), `restore_checkpoint` (SHA-256 dirty-check + `--force`), `list_checkpoints` (metadata), `load_checkpoint`, `latest_checkpoint`, `format_checkpoints`.
8. [P0] Rewrite the two existing checkpoint tests against a temp DB path.
9. [P0] `main.rs`: `Checkpoint { action, id, session, workspace, files, description }`; wire `list`/`create`/`restore`; `--session` defaults to workspace-path hash.

### workflow (P1)
10. [P1] Extract `pub(crate) call_llm_raw(client, model_config, messages)` in `agent.rs`; `Agent::call_llm` delegates; share one `reqwest::Client`.
11. [P1] Add `WorkflowStore` (rusqlite, `workflows.db`, WAL + FK) with `open`/`upsert_run`/`upsert_step`/`list_runs`/`get_run`.
12. [P1] Add `config: Arc<Config>` + `db` fields and `new_with_config` to `WorkflowEngine`; keep `Default`.
13. [P1] Replace the fake spawn closure with `call_llm_raw` + prior-step output injection; persist `StepResult`s in the join loop (off the spawn).
14. [P1] Fix failed-dependency handling (record `Failed`, "skipped: dep failed" instead of "deadlock"); fix `list_runs()`; add `get_run_status`.
15. [P1] Replace `code-review` built-in with a tool-free workflow.
16. [P1] `main.rs`: load config, `Arc`, call `new_with_config`.

### multimodel (P1)
17. [P1] Add `ProviderEntry`/`ModalityRoute`/`RoutingConfig` (struct + field `#[serde(default)]`, `Default`); extend `Config`.
18. [P1] Add `resolve_provider(config, modality)`.
19. [P1] Wire `call_llm`/`call_llm_raw` to use `resolve_provider`; opt-in (empty routing ⇒ current behavior); callers pass `"text"`.
20. [P1] Add a backward-compat YAML round-trip test loading a legacy `[model]`-only config.

### subgoals (P2 — after task 10)
21. [P2] Make `handle_goal_deliberate` call `call_llm_raw` per deliberator role (real output).
22. [P2] Add `Subgoal`; replace `Milestone`; migrate `create_milestone`/`complete_milestone`/`goal_progress`/`format_board`; add `Goal::progress`.
23. [P2] `#[derive(Serialize,Deserialize)]` on `TaskBoard`; add `subgoals` map.
24. [P2] New `goal_store.rs` (atomic JSON via `tempfile::persist()`); `pub mod goal_store;` in `core/mod.rs`; promote `tempfile` to a dependency.
25. [P2] Add `goal_create`/`goal_subgoal`/`goal_list`/`goal_task` tools (handler + `default_tools()` only); persist via `GoalStore`.
26. [P2] `main.rs`: `Goals { action, goal, subgoal, title, description }` command.

### discord (P2)
27. [P2] Add `serenity 0.12` (default-features off, rustls); verify reqwest/TLS dedup + MSRV.
28. [P2] Rewrite `discord.rs`: `Handler`, `EventHandler::message` (bot-ignore, allowlist + dm_policy gate, tokio-Mutex session map with guard dropped across `process().await`, session trim), `start(token, gateway_cfg, model_cfg)`.
29. [P2] `gateway/mod.rs`: uncomment + `tokio::spawn` with error logging.
30. [P2] `config::set`: add `gateway.discord_allowed_users` and `gateway.dm_policy`.

**Shared-dependency note for the implementer:** Task 10 (`call_llm_raw`) gates tasks 19, 21. Build the P1 workflow change first even though multimodel is also P1.


---

# Expert Panel Review (Industry Veteran / Senior Dev / Devil's Advocate)

## Industry Veteran

I have enough grounding. Here is the verdict:

---

## Priority Verdict — openAssistant Six-Feature Review

### Forced Cut Summary

**Ship (P0): update, checkpoint**
**Next sprint (P1): workflow real execution, multimodel config schema**
**Later / deprioritize (P2): subgoals persistence, discord bot**

---

### P0 — Ship in this change

**`update`: Wire `SelfUpdater` in `main.rs`**

This is a one-afternoon task. The struct exists and works. The only gap is ~50 lines in the `Commands::Update` match arm plus two helper methods (`check_pending`, `is_dirty`) in `src/core/self_update.rs`. Do not call `check_update()` at all — the crates.io endpoint will 404. Use git-based version detection only.

One non-obvious trap: drop the `--check` exit-code contract debate until after users actually try this. Ship exit-0-always for MVP; scriptable exit codes are a DX refinement, not user value.

**`checkpoint`: Persist `CheckpointStore` to SQLite**

The current `CheckpointStore` is a useless `Vec` that dies on restart. Every other feature in the system depends on "things survive restart." This is foundational correctness, not a feature. Use `memory.db` (not a separate file — the page-pressure concern for a personal assistant tool is theoretical, not real). Add the two-table schema, wire `open_default()`, done. The lazy-vs-eager API split (Decision B) is easy: return `CheckpointMeta` from `list_checkpoints`, keep `load_checkpoint(id)` separate. Do not add `delete` action yet; `list`, `create`, `restore` are sufficient. Also fix the `main.rs` argument overloading: `--id` is a checkpoint id, add `--session` separately.

---

### P1 — Next sprint, after P0 lands

**`workflow`: Real LLM execution per step**

The DAG topology loop is already correct — this is exactly the MVP-ready infrastructure the research identifies. Go with Option B (lightweight `call_llm_raw` free function, not full `Agent::process()`). The research is right that Option A's `MemoryWorkspace` write-race is a multi-week trap disguised as a small follow-up. Extract `call_llm_raw(model_config: &ModelConfig, messages: &[Value]) -> Result<String>` as a `pub(crate)` free function, thread `Arc<Config>` into spawn closures, add the prior-step context injection, add the `WorkflowStore` SQLite persistence. The built-in `code-review` workflow will produce generic LLM text without tool use — that is acceptable; document it and replace the workflow definition with a simpler "analyze + summarize" pair that does not pretend to need file access.

**`multimodel`: Config schema only, no behavior change**

Add `ProviderEntry`, `ModalityRoute`, `RoutingConfig` to `src/config/mod.rs` with `#[serde(default)]` everywhere. Add `resolve_provider()`. Update `call_llm` to use it. This is a pure backward-compatible pass; old config files are untouched. The vision API branch (`execute_api`) and image_gen stub are P2 — do not ship them alongside the schema change.

The multi-week trap here: the research proposes wiring image_gen, video stubs, and extending `config::set()` all at once. That is scope creep. The schema + `resolve_provider` + `call_llm` wiring is one coherent unit. Everything else waits.

---

### P2 — Defer

**`subgoals`: Goal persistence and the new `Goals` CLI command**

The research is thorough but this is the most over-engineered proposal in the set. The `GoalDeliberator` already returns placeholder text from `agent.rs` — wiring persistence before the deliberator makes real LLM calls produces a system that persists useless stub data. The correct order is: (1) make `goal_deliberate` actually call the LLM (requires `call_llm_raw` from the workflow work above), then (2) persist the output. Starting with a JSON store and the Subgoal struct redesign before the deliberator produces real output is building a database for fake data.

The `Milestone`-to-`Subgoal` replacement is a reasonable cleanup, but do it after `call_llm_raw` exists. Decision 1 (drop Milestone vs keep deprecated): clean path, delete it, the crate has no external users.

**`discord`: serenity integration**

This is the only feature that adds a new major dependency (serenity ~15 transitive crates). The rest of the P0/P1 work uses zero new deps. For a personal assistant tool without a published crate, the Discord bot is a convenience channel, not core value. The research is correct on serenity with minimal features and excluding `cache`. The per-user `HashMap<UserId, SessionState>` design is correct.

The multi-week trap: `MESSAGE_CONTENT` privileged intent requires manual portal configuration that is environment-specific and cannot be tested in CI. The `FullContext` construction needs a verified read of `persona.rs` past line 139 before the handler is written. Session growth (`Session::messages` unbounded) needs a trim call or the bot will silently blow up after long conversations. These are all solvable, but they make this the longest-tail item and the lowest-priority given the other work.

---

### Priority-Ordered Action List

| Priority | Feature | Justification |
|---|---|---|
| P0 | `update`: wire `SelfUpdater` in `main.rs` | ~50 lines, zero new deps, immediate user value, the existing code is done |
| P0 | `checkpoint`: SQLite persistence via `memory.db` | Foundational correctness; every restart currently destroys all state |
| P1 | `workflow`: extract `call_llm_raw`, real LLM per step, `WorkflowStore` SQLite | DAG loop is already right; just the execution is fake |
| P1 | `multimodel`: add `ProviderEntry`/`RoutingConfig` to config + `resolve_provider` | Schema-only, zero behavior change, unblocks everything else |
| P2 | `subgoals`: `Subgoal` struct + JSON persistence + Goals CLI | Depends on `call_llm_raw` (workflow P1) before the deliberator produces real output worth persisting |
| P2 | `discord`: serenity dep + `Handler` + per-user sessions | Correct design but new major dep, manual portal config, not core for a personal CLI tool |

---

### Decisions That Need an Answer Before P1 Starts

1. **`call_llm_raw` visibility**: `pub(crate)` free function in `src/core/agent.rs` (needed by both workflow and multimodel work). Confirm this is the extraction point, not a new module.

2. **`workflows.db` vs `memory.db`**: The research correctly identifies WAL not being enabled on `memory.db`. For workflow step results that can be large strings, use a dedicated `~/.openassistant/workflows.db`. This keeps the memory FTS store clean and lets you `PRAGMA journal_mode=WAL` on each independently.

3. **Drop `Milestone` from `goal_system.rs`**: Yes, clean path, delete it. Zero external users. The struct has no callers outside its own file and no CLI/tool surface.

---

## Senior Dev

Now I have a thorough view of all five features. Here is the technical review:

---

## Technical Review: Five-Feature Research Brief

### Overall Assessment

The research is broadly accurate and grounded in the actual code. The per-file citations are correct, the API shapes match the source, and the "fake vs real" diagnoses are right. The corrections below are specific and consequential — a few would cause compile failures or silent behavioral regressions if left unfixed.

---

### Feature: `update`

**Accurate claims:** `update_from_source()` uses `spawn_blocking` + `Command::output()`, crates.io check is broken for this unpublished crate, the `Update` arm is a stub, `SelfUpdater` is exported and usable.

**Corrections and gaps:**

1. **`ExitStatus::default()` is unstable on `std::process`** — `std::process::ExitStatus` does not implement `Default` in stable Rust. The `unwrap_or` fallback at `self_update.rs:167` will not compile on stable without a workaround. The `run_cmd` helper already exists and has this latent bug; any new helper (`check_pending`, `is_dirty`) must not rely on `ExitStatus::default()` — use `unwrap_or_else(|_| ...)` with a hand-constructed `Output` using `ExitStatus` from a real process, or just propagate the error. The research does not flag this.

2. **`--check` flow uses `git fetch` — this mutates remote-tracking refs.** The research says "Run `git fetch` then check `git log HEAD..origin/HEAD`". On Windows, `git fetch` over HTTPS may trigger a credential prompt or fail silently if no network. The safer pattern is `git fetch --dry-run` for "is there anything to fetch", but even that isn't a real check without actually fetching. Recommend `git fetch` (accept the state-mutating effect) but document the network dependency clearly; `--check` is not purely read-only.

3. **Workspace dir detection via exe-parent walk is unreliable on Windows.** `std::env::current_exe()` on Windows often resolves to `target\release\openassistant.exe` inside the repo, which is correct. But if the binary is installed to `%USERPROFILE%\.cargo\bin\`, the exe-parent walk will never find a `Cargo.toml` because the binary has been copied. The `OPENASSISTANT_SRC` env var escape hatch is essential for any non-developer install. The research describes this correctly but understates the probability: most non-developer users will not have the repo cloned. This is a feature that only works for source installs — document that at the top of the `Update` match arm with a clear error message.

4. **`Permission::Ask` in `update_from_source()` — the check is `!= Deny`, not `== Allow`.** The existing code at line 63 proceeds if `permission != Deny`. Since the default is `Ask`, it will proceed without prompting. The research says "no path to set `Allow` from the CLI" but misses that the code already skips the confirmation when `Ask`. The new CLI arm needs to implement the `--yes` / stdin prompt **before** calling `update_from_source()`, and pass the method `Permission::Allow` only after confirmation, or the method's permission check is meaningless.

5. **No concern about replacing a running binary on Windows — the research mentions it but calls it unavoidable.** It is worth noting that `cargo build --release` writes to `target/release/openassistant.exe`. If the user ran the update command via the installed binary at `%USERPROFILE%\.cargo\bin\`, the running exe is a *different file* from the one being built. The user still needs to either `cargo install --path .` or manually copy the new binary. The research's recommendation to print "Restart openassistant to use the new binary" is insufficient — it should say "then run `cargo install --path .` to install the new binary to your PATH".

---

### Feature: `workflow`

**Accurate claims:** DAG loop is correct, fake closure identified, `call_llm` loads config on every turn, `rusqlite` is available, `!Send` constraint on `Connection` is correctly flagged, `list_runs()` returning empty vec.

**Corrections and gaps:**

1. **`WorkflowRun` and `StepResult` do not derive `Serialize`/`Deserialize`.** The research proposes mapping them to SQLite, but these structs (lines 45–65 of `workflows.rs`) derive only `Debug, Clone`. Adding `Serialize`/`Deserialize` is not needed for rusqlite (which uses column extraction, not serde), but the research should not imply these can be trivially stored — you need explicit `row.get::<_, String>(0)` pattern, not a serde-based approach. Verify before writing schema code.

2. **`WorkflowStatus` derives `Serialize, Deserialize, PartialEq`** (line 36) but `WorkflowRun` does not (line 45). This is inconsistent in the codebase but the research does not note that the status-to-TEXT mapping needs an explicit `match` when doing INSERT — `serde_json::to_string()` would give `"Completed"` in quotes, not `Completed`. Use `format!("{:?}", status)` or implement `Display`.

3. **`call_llm` is a private `async fn` on `Agent` (`src/core/agent.rs:178`).** Extracting it to `pub(crate) async fn call_llm_raw` is the right approach. However, `call_llm` uses `self.model: String` at line 183 — `body["model"] = self.model`. In `call_llm_raw`, the model must come from `resolve_provider` (multi-model feature) or from `config.model.model` directly. The research says `call_llm_raw` should accept `&ModelConfig` + messages, but `ModelConfig` does not implement `Clone` — check before assuming it can be cheaply wrapped in `Arc`. Actually `ModelConfig` has only `String` fields; it trivially derives `Clone` (line 56 `Cargo.toml` confirms `serde`). This is fine.

4. **`tokio::spawn` in `execute()` for the fake closure is `'static + Send`**. The real `call_llm_raw` call is also `Send` (returns `String`, uses `reqwest`). No compile barrier there. But the research should note that `reqwest::Client` should be constructed once (outside the spawn) and cloned into each closure — constructing a new `Client` per step is expensive (connection pool per client). The existing `call_llm` creates a new `reqwest::Client::new()` at line 181 per call — this is an existing bug that should be fixed at the same time `call_llm_raw` is extracted. Pass an `Arc<reqwest::Client>` through.

5. **`WorkflowEngine::new_with_config` vs. keeping `Default`:** the research proposes removing `Default`. That breaks `Commands::Workflow` in `main.rs` (line 272 constructs `WorkflowEngine::new()`). Keep `Default` and add `new_with_config` as an additional constructor. The CLI arm can keep using `new()` for the stub path; real execution uses `new_with_config`.

---

### Feature: `checkpoint`

**Accurate claims:** `Vec<Checkpoint>` confirmed, `max_checkpoints = 50`, existing tests use `tempfile`, `rusqlite` is in `Cargo.toml`, `restore_checkpoint` writes files correctly.

**Corrections and gaps:**

1. **`list_checkpoints` returns `Vec<&Checkpoint>` — the proposed `CheckpointMeta` split breaks existing callers.** The only external caller of `list_checkpoints` is `format_checkpoints` (same file) and the `main.rs` `Checkpoint` arm. `latest_checkpoint` (line 115) returns `Option<&Checkpoint>` and is also local. There are no external callers in `agent.rs` despite the research claiming "agent.rs calls `latest_checkpoint`". Verify: agent.rs does not call `CheckpointStore` at all. The API split is a non-issue for MVP — the research correctly defers it.

2. **`ON DELETE CASCADE` requires `PRAGMA foreign_keys = ON` on every connection, not just once.** SQLite disables foreign keys by default and resets the pragma per-connection. The research mentions this correctly, but it needs emphasis: the `open()` constructor must run `PRAGMA foreign_keys = ON` immediately after `Connection::open()`. The existing `MemoryStore::open()` does NOT enable foreign_keys — confirm this before proposing to reuse the same connection pattern verbatim.

3. **`rusqlite::Connection` is `!Send` — the `main.rs` checkpoint arm is inside `#[tokio::main]`.** The research says "opening `CheckpointStore` synchronously in the `Commands::Checkpoint` match arm is safe". This is correct only if you never `.await` while holding the `Connection`. In the proposed design, `CheckpointStore::open_default()` is `fn` (sync) and all CRUD is sync — this is fine. Do not accidentally make any checkpoint method `async` while also holding the connection, or you'll need to either `spawn_blocking` or wrap in `Arc<Mutex>`.

4. **`file_snapshots: HashMap<String, String>` — the research proposes storing full file text as TEXT in SQLite.** This works for source code but fails for binary files (images, compiled artifacts). The existing `create_checkpoint` uses `std::fs::read_to_string` (line 53), so it already silently drops binary files. The SQLite `content TEXT` column has the same limitation. Document this explicitly: checkpoint only works for UTF-8 text files. Non-UTF-8 files are silently skipped, consistent with existing behavior.

5. **The `--session` / `--id` clap arg split conflicts with the existing `Checkpoint` variant.** `main.rs:62` has `Checkpoint { action: Option<String>, id: Option<String> }`. Adding `--session` and `--description` requires changing the enum variant, which is a source-compatible change (adding fields with `Option` default). But the existing `list` action uses `id` as session-id (a named overload). The research recommends splitting them, which is correct. Just flag that the existing `--action list --id <session-id>` usage (line 287) breaks after the refactor — users with scripts must update.

---

### Feature: `discord`

**Accurate claims:** serenity is the right crate, `is_allowed` function exists and is correct, discord::start is a stub, `gateway/mod.rs` line 19 has the commented-out call, `MESSAGE_CONTENT` is a privileged intent.

**Corrections and gaps:**

1. **serenity version: the research says "0.12.5 (released 2025-12-20)".** The latest serenity on crates.io as of mid-2025 is `0.12.x` — do not specify a patch version in `Cargo.toml`. Use `serenity = { version = "0.12", ... }` with the semver range, not `"0.12.5"`. The research's `Cargo.toml` snippet already does this correctly; the contradiction is only in the prose.

2. **`reqwest` conflict risk is LOW but not zero.** The research says serenity vendors its own reqwest. This is true — serenity 0.12 uses `reqwest` as a normal crate dep, not a vendored copy. Cargo will unify to the single version that satisfies both `open-assistant`'s `reqwest = "0.12"` and serenity's reqwest constraint. If serenity 0.12's reqwest dep is also `"0.12"`, they unify cleanly. If serenity pins to `"0.11"`, there is a duplicate. Check `serenity 0.12`'s `Cargo.toml` before claiming zero risk; the research should say "likely unified" not "compiled separately".

3. **`Arc<Mutex<HashMap<u64, SessionState>>>` — `Mutex` here is `std::sync::Mutex` or `tokio::sync::Mutex`?** The research references `tokio::sync::Mutex` for the sessions map. Inside serenity's `async_trait` event handler, `.lock().await` on a `tokio::sync::Mutex` is correct. But `std::sync::Mutex::lock()` also works if the lock is released before any `.await` point. For the session map access pattern (lock, get-or-insert, call `agent.process()`, release), the lock must be released before `agent.process().await`. That means: lock, clone or get the state, release the lock, call process, re-lock to update. A `tokio::sync::Mutex` makes this easier (`lock().await` + dropping the guard before await). The research implies the right pattern but does not make this explicit — locking across `agent.process().await` while holding the `tokio::sync::Mutex` guard will cause a compile error in the async context.

4. **`Handler.agent: Agent` is not `Sync`.** serenity's `EventHandler` requires `Handler: Send + Sync + 'static`. `Agent` is `Clone` (line 14 of agent.rs: `#[derive(Debug, Clone)]`) but whether it is `Sync` depends on its fields. `Agent { model: String, tools: Vec<ToolDefinition>, workspace_dir: String, tools_enabled: bool }` — all fields are `Sync`. So `Agent: Sync`. No issue, but the research does not verify this. Worth confirming before wiring.

5. **Per-user `FullContext` construction — `Persona::load_or_default` API not verified.** The research says `FullContext { persona: Persona::load_or_default(&data_dir), user_model: UserModel::default(), ... }`. Before implementing, read `src/core/persona.rs` to verify `FullContext`'s actual field names and that `Persona::load_or_default` exists with this signature. The research does not cite a line number for `FullContext` construction, which is the field most likely to have undocumented required fields.

6. **`gateway/mod.rs` calls `webchat::start(config.gateway.webhook_port).await?` on the main task.** If discord is spawned first and webchat runs on the main task, they run concurrently — correct. But `webchat::start()` is a long-running `axum` server that blocks forever. The `start_gateway()` function currently does `webchat::start(...).await?` as the last line, which means it never returns. The Discord spawn is fire-and-forget (no join, no error propagation after spawn). If Discord panics, it is silently lost. The research notes this pattern but does not flag the error-propagation gap. Use `tokio::select!` or a `JoinSet` if crash propagation matters.

---

### Feature: `subgoals`

**Accurate claims:** `Milestone` struct confirmed, `TaskBoard` has no `Serialize`/`Deserialize`, `GoalDeliberator` is ephemeral, no CLI command exists, `Task.parent_id` unused.

**Corrections and gaps:**

1. **`TaskBoard` derives `Debug, Default` but NOT `Serialize, Deserialize` (line 374 confirmed).** Adding these derives requires that `HashMap<String, Task>` and `HashMap<String, Goal>` are also serializable — they are, since `Task` and `Goal` already derive `Serialize, Deserialize`. This is straightforward but the research understates the impact: adding `#[derive(Serialize, Deserialize)]` to `TaskBoard` will also pick up any non-serializable nested types. `Goal.milestones: Vec<Milestone>` — `Milestone` derives `Serialize, Deserialize` (line 364 confirmed). Clean.

2. **`GoalStatus` derives `PartialEq` (line 355) but `Priority` does NOT derive `PartialEq` — wait, actually it does** (line 305: `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]`). The research proposes reusing `GoalStatus` for `Subgoal.status` — this is valid.

3. **Dropping `Milestone` requires updating `goal_progress()` which currently drives off `goal.milestones.len()` (line 539).** If `Milestone` is removed and replaced with `Subgoal`, the `format_board()` output that reads `({}/{} milestones)` (line 596) must also change. The research identifies this but the `format_board()` function is not in the files-to-touch list. Add it.

4. **`serde_json` is not a direct dependency in `Cargo.toml` for the JSON persistence approach.** Wait — `serde_json = "1"` IS in `Cargo.toml` line 31. The research correctly notes "zero new dependencies". Confirmed.

5. **`GoalStore` as a new file** requires re-export through `src/core/mod.rs` (not just `src/lib.rs`). The research says touch `src/lib.rs` but the core module tree is `pub mod core` in `lib.rs` and individual submodules in `core/mod.rs`. A new `goal_store.rs` needs `pub mod goal_store;` in `src/core/mod.rs`. The research's file list says "src/lib.rs — Re-export `goal_store` module" which is slightly wrong — the correct touch point is `src/core/mod.rs`.

6. **The three tool touch-points rule (handle_tool_calls + default_tools() + ToolRegistry::execute):** The research correctly identifies all three. But `src/tools/mod.rs` currently has no goal entries — the new `goal_create`/`goal_list`/etc. tools need to be added only to `handle_tool_calls` in `agent.rs` and `default_tools()`. They do NOT need a `ToolRegistry::execute` entry unless the tools need to be callable from `ToolRegistry` directly (which the other agent-internal tools like `todo_write` and `goal_deliberate` do not use). Check whether `ToolRegistry::execute` is in the dispatch path for `[TOOL:goal_create:...]` calls — if `handle_tool_calls` catches them first (which it does, line 94–98 pattern), `ToolRegistry` is not invoked. The research correctly says "only `agent.rs` needs updating" but then contradicts itself by listing `src/tools/mod.rs` in the touch list.

---

### Feature: `multimodel`

**Accurate claims:** `ModelConfig` fields confirmed, `call_llm` loads config on every turn using `config.model.*`, `Agent.model` is set at construction, `vision.rs` hardcodes `"gemini"`, `base64 = "0.22"` is present, `config::set()` allowlist confirmed missing `model.api_base`.

**Corrections and gaps:**

1. **`#[serde(default)]` on new `Config` fields is necessary but not sufficient for backward compatibility.** The existing `ModelConfig` struct at line 56 does NOT have `#[serde(default)]` on the struct itself — if a user's `config.yaml` has the `[model]` section but is missing a field, deserialization will fail. Adding `providers` and `routing` to `Config` with `#[serde(default)]` is correct. But if any field inside `ProviderEntry` or `RoutingConfig` lacks a default and the user hand-edits an incomplete entry, load will fail. Every new struct needs `#[serde(default)]` on the struct-level attribute AND `Default` impls. The research provides this but it should be called out as a load-or-fail risk during the config YAML round-trip.

2. **`call_llm` uses `self.model` at line 183 (`body["model"] = self.model`).** The proposed change passes `model_name` from `resolve_provider`. But `Agent::process()` calls `self.call_llm(&messages).await?` — with the new `modality` parameter it becomes `self.call_llm(&messages, "text").await?`. Every call site must be updated. The research lists this at P0 item 3: "all internal callers pass `'text'`" — correct. But there is exactly one call site (`agent.rs:94`) plus any tests. Low risk but must be exhaustive.

3. **`resolve_provider` returns `(&str, &str, &str)` with lifetime tied to `&'a Config`.** Inside `call_llm`, `config` is a local variable (loaded from disk on line 179). The `&str` lifetimes from `resolve_provider` are tied to `config`'s lifetime, which is the function body. This is fine — `config` outlives the `reqwest` call. But if `resolve_provider` is later refactored to take the config by `Arc`, the lifetime story changes. The function signature is sound for now.

4. **`config::set()` does not support `model.api_base` (confirmed in source, line 176–188).** The research calls this out correctly. However, the practical fix for MVP is not to add it to the allowlist — it's to document that multi-provider config must be set by editing `~/.openassistant/config.yaml` directly. The allowlist is a security-by-design choice (prevents injecting arbitrary base URLs via a single CLI flag), and expanding it for `routing.*` and `providers.*` opens up multi-URL injection. Flag this as a security consideration, not just a functionality gap.

5. **`vision.rs` hardcodes `"gemini"` and ignores `config.vision.gemini_path`.** Confirmed from source (line 25: `Command::new("gemini")`). The fix is `Command::new(&config.vision.gemini_path)`. But `execute()` currently takes `args: &serde_json::Value` — it does not accept a `Config` reference. To fix the path, either change the signature to `execute(args, config)` or read the config inside the tool. The research proposes the `execute(args, config)` form, which is the cleaner approach. But this is a **signature change** on a public function — verify all callers. There is one caller: the `vision` arm in `handle_tool_calls` (agent.rs ~line 327). Pass `&config` there. Note that `config` is already loaded earlier in `call_llm`'s scope but `handle_tool_calls` is a separate method — it will need to load config or accept it as a parameter. Currently `handle_tool_calls` at agent.rs ~line 260 does not take a config argument.

6. **`Decision A` (Option 1 vs Option 2 for modality dispatch):** The research recommends Option 1 (vision tool uses `resolve_provider` directly, bypassing `call_llm`). This is correct for now, but creates a second `reqwest::Client` construction path. The more important correction: if the `reqwest::Client` is extracted and shared (as recommended under Workflow above), the vision API branch needs access to the same shared client. Plan the client refactor alongside the multi-model work.

---

### Cross-Cutting Issues Not Raised in the Research

1. **`config::load()` called multiple times per process in the same request.** `call_llm` loads config (line 179), `start_gateway` loads config (mod.rs line 14), `main.rs` loads config in multiple arms. Each load does a `tokio::fs::read` + YAML parse. This is not a correctness issue but is a performance smell that multiple features aggravate. A process-level `OnceLock<Config>` with a reload-on-SIGHUP pattern would be the standard fix, but this is pre-existing and out of scope for any single feature.

2. **`ExitStatus::default()` — recurs in `self_update.rs:167`.** The `spawn_blocking` unwrap fallback constructs a fake `Output` with `ExitStatus::default()`. On stable Rust, `ExitStatus` does not implement `Default`. This will produce a compile error when the file is actually compiled with the `update` command wired (currently the `update` arm never calls `SelfUpdater`, so this code may not be exercised). Verify with `cargo check` immediately when wiring.

3. **`serenity 0.12` MSRV.** serenity 0.12 requires Rust 1.74+. The project's `Cargo.toml` does not specify a `rust-version` field. Confirm the CI/build environment meets this before adding the dep.

4. **`serde_json` in `Cargo.toml` is version `"1"` (line 31), not a fixed minor.** All new serialization code is safe to use with this.

5. **`config = "0.15"` is listed as a dep (line 61) but the codebase does not appear to use it** (config loading is custom YAML via `serde_yaml`). This is a pre-existing dead dep, not introduced by these features.

---

## Devils Advocate

Now I have a solid grounding in the actual source. Here is my Devil's Advocate critique of each proposal:

---

# Devil's Advocate Review — openAssistant Feature Proposals

## Feature: `update`

### Claim: "exe-parent walk to find Cargo.toml" is reliable on Windows

**Rejected.** The installed binary on Windows is typically at `%USERPROFILE%\.cargo\bin\openassistant.exe`. Walking upward from there reaches `%USERPROFILE%\.cargo\bin\` → `%USERPROFILE%\.cargo\` → `%USERPROFILE%\` — none of which contain the source `Cargo.toml`. The walk silently hits the filesystem root and fails. The `OPENASSISTANT_SRC` env var becomes *mandatory* for virtually every Windows user, but the brief presents it as an "escape hatch." That inversion should be stated plainly: **the exe-parent walk is Linux/Mac-specific; on Windows only the env var works.** The fallback error message therefore needs to be the primary user-facing path, not the exception.

### Claim: "`check_update()` just won't be called" — the crates.io dead end

The brief correctly notes `check_update()` is broken (crate not published, will 404), but it leaves it in place undocumented. That is a trap for the next developer. The function has a misleading comment ("Compare with crates.io or git"). Either delete it or replace its body with `anyhow::bail!("not published to crates.io; use check_pending() for git-based check")`. Leaving dead code that silently returns `Ok(None)` on a 404 is the same as a lie.

### Claim: "`git pull --rebase` then `cargo build --release` — the happy path"

**Unverified assumption: `git` and `cargo` are on PATH.** The brief assumes both are available. On a user's production machine (not a developer workstation) `cargo` may not be in PATH even if Rust is installed via `rustup`, because `~/.cargo/bin` is only in PATH after sourcing the shell profile. On Windows with PowerShell, `$env:PATH` for a child process launched from the binary may not include `~/.cargo/bin`. `run_cmd` uses `std::process::Command` which inherits the binary's environment — if the binary was launched from a shell that has `~/.cargo/bin`, it works; if launched from a GUI launcher or a service, it silently fails. The brief does not mention this. At minimum, the error from a `Command::new("cargo")` failure should produce a message like "cargo not found — ensure Rust/Cargo is on PATH."

### The `spawn_blocking` + `ExitStatus::default()` silently-swallows bug

The brief correctly flags this: the `unwrap_or(Ok(ExitStatus::default()))` in `run_cmd` (line 167 of `self_update.rs`) means a `JoinError` (e.g., the blocking thread panics because `cargo build` triggers an OOM or filesystem error) is replaced with a fake "success" exit status. The fix is not just documenting it — the brief says it is "worth documenting." That is insufficient. A panicking `cargo build` must not be reported as success. Fix: `spawn_blocking(...).await??` (propagate both the `JoinError` and the `io::Error`), replacing the `unwrap_or` entirely.

### The `--yes` flag skips the dirty-tree check

The proposed flow gates dirty-tree detection before `update_from_source()`. But `update_from_source()` has its own permission check (`Permission::Ask`) with no path to `Allow`. The brief proposes `--yes` as a confirmation skip, but the `SelfUpdater.permission` field is always `Ask` and there is no setter path from the CLI arm. This means `update_from_source()` would proceed regardless of `permission` when called directly from the CLI arm — the `permission != Deny` check in the existing function would pass. The `SelfUpdater.permission` field is effectively dead code in this wiring. Document or remove it.

### Windows: replacing a running `.exe`

The brief notes this but understates the severity. `git pull` modifies source files in the repo; `cargo build --release` writes a *new* `target/release/openassistant.exe`. On Windows the **new** binary is not the running one, so the write succeeds. On Unix the same is true for `target/release/openassistant`. The *actual* problem is subtler: if the user originally installed via `cargo install`, the installed binary is at `~/.cargo/bin/openassistant`. The build step puts the new binary in `target/release/`, not in `~/.cargo/bin/`. The update is silent and the user keeps running the old binary until they run `cargo install --path .` manually. The brief says "Restart openassistant to use the new binary" — but restarting opens the *same* binary, not the newly built one, unless the binary was run from `target/release/` directly. This is a significant workflow gap that needs calling out explicitly, not buried in a risk note.

---

## Feature: `workflow`

### Claim: "the dependency-DAG loop is correct"

Mostly true per the source, but there is a **silent deadlock mishandling**. Line 134: `return Err(anyhow::anyhow!("Workflow deadlock"))` — this fires when `ready.is_empty()` but `remaining` is non-empty. What it does not detect: a step that depends on a step that *failed* (error path on line 172, `tracing::warn` and no insertion into `completed`). If step A fails and step B depends on A, B is never inserted into `completed`, `remaining` never shrinks past B, and the loop detects a "deadlock" rather than "step B was skipped because A failed." The error message "Workflow deadlock" is misleading; the real condition is "unsatisfiable dependency." The brief does not mention this.

### Claim: "Option B (raw `call_llm`) avoids MemoryWorkspace race"

But Option B requires extracting `call_llm` as a free function, because `call_llm` is currently `async fn call_llm(&self, messages)` — a private method on `Agent`. The brief acknowledges this ("requires extracting `call_llm_raw`") but glosses over the actual change: `call_llm` calls `crate::config::load().await?` internally. If N steps run in parallel, each spawns a tokio task that calls `config::load()`, which calls `tokio::fs::read_to_string(config_path)` — N concurrent reads of the same YAML file. On Windows, `tokio::fs::read_to_string` uses `OpenOptions` without `FILE_SHARE_READ` (actually it does share reads by default on Windows — low risk in practice, but not zero, especially if another process writes the config concurrently, e.g. the user runs `openassistant config --key ...` in a separate terminal while the workflow runs).

### Claim: "`rusqlite::Connection` kept on the caller side — correct"

True that the pattern is established in `MemoryStore`. But the brief says "write results after the `handle.await` loop" — this works for the inner per-wave loop but not for a crash mid-workflow. If the process is killed between wave 1 completing and the SQLite write, wave 1 results are lost. The proposed schema has no "partial completion" recovery. For a feature being pitched as replacing ephemeral in-memory results with persisted state, losing all results on an unclean exit is a substantial limitation that the brief understates.

### The built-in `code-review` workflow is meaningless without tool execution

The brief acknowledges this but calls it "a demo limitation." It is worse than that — the `explore` step says "Explore codebase and find changed files" and the LLM, given no context and no tools, will either hallucinate file names or produce generic boilerplate. If a user runs `openassistant workflow code-review` after this change, they will get confidently wrong output for the most prominent built-in workflow. The brief should either (a) delete the `code-review` built-in from `built_in_workflows()` for the MVP, replacing it with something that actually works without tool access (e.g., "summarize the README"), or (b) explicitly mark it as `[DEMO ONLY — requires tool dispatch]` in the output. Silently returning hallucinated "findings" is worse than the current honest stub.

### `WorkflowEngine::new_with_config` vs existing `Default` impl

The brief proposes renaming the constructor to `WorkflowEngine::new_with_config`. The current code has `impl Default for WorkflowEngine` which calls `Self::new()`. Any call site using `WorkflowEngine::default()` would still compile but produce an engine without a config, silently falling back to... no config at all. The brief does not address how the `Default` impl should behave after this change. If `Default` is kept as-is (no config, which means no `call_llm_raw` can work), then the first real LLM call will fail with a missing-config error at runtime, not at construction time. Better: either remove the `Default` impl and require the config explicitly, or have `Default` panic with a clear message.

---

## Feature: `checkpoint`

### The doc comment says "Uses SQLite" but the code uses `Vec<Checkpoint>`

The brief correctly identifies this gap, but misses a consequent problem: the **existing tests** in `checkpoint.rs` (lines 171–214) test the in-memory `Vec` implementation and will pass. After the SQLite rewrite, those same tests must be rewritten to use a temp DB path (`tempfile::NamedTempFile` or `tempfile::tempdir()`). The brief does not mention this. If the tests are not updated, they will test the old Vec code path while the production path uses SQLite — giving false confidence.

### Claim: "reuse `memory.db` via new tables — recommended"

The brief's own counter-argument is stronger than its recommendation: file snapshots can be multiple MB. `checkpoint_files.content TEXT NOT NULL` storing many MB of file text in the same SQLite file as the FTS5 memory store creates real page-cache pressure and makes `VACUUM` slow. More importantly, `MemoryStore::open()` in `src/memory/store.rs` is an `async fn` that creates `memory.db` with its own schema. The checkpoint code would need to either (a) open a separate connection to the same file (two concurrent open connections with WAL — workable but WAL is not currently enabled, as the brief notes), or (b) call into `MemoryStore` to create its tables (coupling). The brief's own advice says "Enable cascade by running `PRAGMA foreign_keys = ON`" but says nothing about `PRAGMA journal_mode=WAL` which is needed for safe concurrent access. The separate `checkpoints.db` path is actually less risky for MVP. The brief says "Decision A — verdict needed" but then leans toward the riskier option.

### The `restore_checkpoint` writes files without confirmation

`restore_checkpoint` at line 89–98 uses `std::fs::write(&full_path, content)` — unconditional overwrite. If the user's working directory has unsaved changes in those files, restore silently clobbers them. The brief does not mention a dirty-check before restore. The `create_checkpoint` path reads files and computes SHA-256 hashes — those hashes exist in `file_hashes`. Before overwriting during restore, the code could compare `sha2::Sha256::digest(current_content)` against the snapshot hash and warn/skip if the file has been modified since the checkpoint. The brief completely ignores this data-loss vector.

### `session` identifier design — the brief's proposal breaks existing test

The brief proposes adding `--session` for session ID to replace the current `--id` which is overloaded. But the current `Checkpoint` struct has `session_id: String` with no default format. The brief proposes "a stable ID derived from the workspace path" as a default — a hash or path string. If it uses a hash, it's opaque; if it uses the path string, it changes when the user renames their project directory, orphaning all prior checkpoints. The brief does not pick a format and does not explain migration from the current scheme (where the CLI user had to pass an arbitrary `--id` string as session ID).

---

## Feature: `discord`

### Claim: "serenity's internal reqwest vs the project's reqwest — Low risk"

The brief says "both use reqwest 0.12" and "cargo deduplicates." This is only true if serenity's internal reqwest feature flags are compatible. Serenity's `http` feature enables reqwest with specific features (TLS backend, JSON, etc.). If serenity's reqwest needs `native-tls` and the project uses `rustls`, both will be compiled — no deduplication, two TLS stacks in the binary. The brief specifies `rustls_backend` for serenity, which should prevent this, but this claim is asserted without verifying serenity's actual `Cargo.toml`. It needs explicit verification before calling it low risk.

### Claim: "`Agent::process()` requires `&mut FullContext` and `&mut Session`"

The per-user `Arc<Mutex<HashMap<UserId, SessionState>>>` requires holding the lock across the `await` of `agent.process()`. This is exactly the **async Mutex held across await** antipattern. Holding a `tokio::sync::Mutex` guard across an HTTP call (which is what `call_llm` does) is not a deadlock by itself (tokio's Mutex is async-safe), but it means one user's long-running LLM call blocks all other users from getting their session state. For a personal-assistant bot with one owner this is acceptable, but the brief presents this design without flagging the async-Mutex-across-await pattern at all. It should be named explicitly so implementers don't accidentally use `std::sync::Mutex` (which *would* deadlock).

### Privileged Intent: the brief says "enable in the Developer Portal" — this is a build-time invariant, not a runtime one

If `MESSAGE_CONTENT` is not enabled in the portal, `msg.content` is empty and the bot calls `agent.process("")` — which calls `call_llm` with an empty user message. The LLM responds to an empty prompt with something generic. The bot then sends this response to the Discord channel. The user sees the bot responding nonsensically with no error, and there is no runtime log warning the operator. The brief says "log a warning at startup if the bot is in any guilds" — but that requires reading guild membership, which requires the `cache` feature. Without `cache`, serenity cannot enumerate guilds at startup. The brief excludes `cache`. This is an unresolved contradiction.

### `FullContext` construction: the brief says "verify the exact `FullContext` struct fields (read `persona.rs`)"

This is not a verified claim — it is deferred work punted to the implementer. The brief should have read `persona.rs` and verified whether `FullContext::new()` / `Persona::load_or_default()` exist and what their signatures are. Asserting "each user gets their own `FullContext`" without verifying the constructor is hand-wavy.

### Session memory growth is understated

The brief flags "Session::messages is unbounded" and says `call_llm` trims to last 30 for the HTTP request. But the in-memory `Session` struct keeps accumulating all messages indefinitely. For a Discord bot that runs continuously (unlike the CLI which exits after each invocation), a user with a long conversation will accumulate megabytes in-process. The brief calls this a P1 concern. It should be P0 for a long-running service, or at minimum the per-user session should be bounded at construction time.

---

## Feature: `subgoals`

### The entire feature has zero users today — adding it has zero leverage

`GoalDeliberator.handle_goal_deliberate()` in `agent.rs` (line 296 in the tool dispatch) returns a placeholder string and does not persist anything. The `goal_deliberate` tool is registered in `default_tools()` and thus advertised to the LLM in every system prompt. If the LLM invokes it, the user gets "In a full implementation..." text. The subgoals proposal adds a new JSON file, a new `Subgoal` struct, a new `GoalStore`, CLI commands, and four new agent tools — all before fixing the fundamental problem that `GoalDeliberator` makes no real LLM calls. Adding a persistence layer to an ephemeral stub that produces placeholder text provides zero user value. The correct order is: first make `GoalDeliberator` call the LLM for each role; then worry about persisting the outputs.

### Claim: "`Milestone` can be dropped — the crate has no external dependents"

True today, but `Milestone` is referenced in `goal_progress()` and `format_board()` in `goal_system.rs`. Deleting it requires verifying all callers in the codebase. The brief says "zero external users today" based on the crate being unpublished — but internal callers exist. The brief does not enumerate them. `format_board()` in particular returns a formatted string used in... the brief does not say where `format_board()` is called. This is unverified.

### JSON atomic write is not atomic on Windows

The brief says "atomic write: serialize to string, `std::fs::write(tmp) + rename`." But `std::fs::write` is a single-step overwrite (open → truncate → write). The "tmp + rename" pattern requires manually creating a temp file, writing to it, then `std::fs::rename`. `std::fs::write` does not do this. On Windows, `rename` across drives fails with `io::Error` code 17 (`ERROR_NOT_SAME_DEVICE`) if the temp file and target are on different drives — rare but possible if `%TEMP%` is on a different volume. The brief presents this as a solved problem when it is an implementation detail that requires explicit `tempfile` crate usage or a manual `NamedTempFile` + `persist()` pattern. If the process crashes mid-write, `goals.json` is truncated to zero and all goals are lost.

### No locking between concurrent CLI invocations

A user running `openassistant goals --action create` in two terminal tabs simultaneously will experience a read-modify-write race on `goals.json`. Both processes read the file, both modify in memory, both write — last writer wins, one goal is lost. The brief does not mention file locking. `rusqlite` handles this via WAL; `goals.json` does not. This is another argument against JSON for MVP, but the brief recommends JSON without flagging the concurrency hazard.

---

## Feature: `multimodel`

### `ModelConfig` has no `#[serde(default)]` at the struct level

The brief proposes adding `providers: Vec<ProviderEntry>` and `routing: RoutingConfig` with `#[serde(default)]`. But `ModelConfig` itself does **not** have `#[serde(default)]` at the struct level (verified in `src/config/mod.rs` lines 55–61). If a user's `config.yaml` from before this change has a `model:` section without `api_base` (e.g., they removed it), `serde_yaml` will fail to deserialize the entire `Config` because `api_base` has no `#[serde(default)]`. The existing round-trip test passes only because it starts from `Config::default()` which has all fields. Real migration risk exists. The brief does not address backward compatibility for the existing `ModelConfig` struct's required fields.

### Claim: "Option 1 (two separate HTTP paths for text and vision) is cleaner"

The brief recommends Option 1: `call_llm` stays text-only; vision uses its own `reqwest` client. This means two separate places that construct `reqwest::Client`, build auth headers, POST to an LLM endpoint, and parse the response JSON. Every bug fix or header addition (e.g., adding `X-Request-ID` for debugging) must be applied in two places. This is not "cleaner separation" — it is duplication. The right abstraction is a single `LlmClient::post(messages, modality)` free function, not two independent HTTP call sites. The brief presents this as a design decision but does not acknowledge the duplication hazard.

### `resolve_provider` returns borrowed `&str` from `config` — lifetime issue

```rust
pub fn resolve_provider<'a>(config: &'a Config, modality: &str) -> (&'a str, &'a str, &'a str)
```

When the fallback branch returns `(&config.model.api_base, &config.model.api_key, &config.model.model)` — all fine, lifetimes are valid. But `call_llm` currently calls `let config = crate::config::load().await?` which returns an owned `Config`. Storing `config` as a local, then calling `resolve_provider(&config, ...)` to get `(&str, &str, &str)` references into it, works because `config` outlives the references within the function scope. This compiles. However, the brief describes these as `&'a str` returns — the caller must keep `config` alive. In `call_llm`, `config` is a local variable that lives until end of function, and the resolved strings are consumed within the same function. This is fine — but the brief's description of the function signature implies it could be used as a cached resolver, which it cannot be without holding the `Config`. This is an API design smell worth calling out.

### The existing `call_llm` ignores `self.model` in favor of `config.model.model` on every call

Actually false — looking at `agent.rs:182`, `call_llm` uses `self.model` for the `"model"` field in the JSON body, and `config.model.api_key` / `config.model.api_base` from the loaded config. So the model string comes from `self` (set at `Agent::new(model)` time), but credentials come from disk. After the proposed refactor using `resolve_provider`, the model string would *also* come from disk (from `routing.text.model`). This breaks the current behavior where `Agent::new("some-model")` at the call site controls which model runs. Any code that explicitly constructs `Agent::new("claude-3-opus")` would be silently overridden by the config's routing table. The brief does not flag this behavioral change, which affects all three `Agent::new` call sites in the desktop commands.

---

## Cross-Cutting Issues the Research Missed Entirely

1. **No error surfacing strategy.** Every proposal adds new error paths (SQLite open failure, git not found, Discord token invalid, goals.json corrupted). None of the briefs specify how errors are presented to the user in the TUI or desktop UI. The CLI match arms currently use `println!("❌ ...")` — but the new code paths use `?` propagation, which in `main()` exits with a Rust error message, not a friendly CLI message. The existing checkpoint arm at line 284 calls `CheckpointStore::new()` (infallible) — after the SQLite rewrite, `open_default()` is fallible; the `?` in the match arm would exit the process on a DB error. Is that the right UX?

2. **The `ExitStatus::default()` on Windows is not what you think.** `std::process::ExitStatus::default()` is Unix-only stable behavior (exit code 0). On Windows, `ExitStatus::default()` panics in Rust's standard library before Rust 1.60 and is `ExitStatus(0)` in later versions — but `0` means *success* on Windows too, so a swallowed `JoinError` still appears as success. The fact that the project targets Windows (confirmed by the platform field in the env block) makes this particularly important to fix.

3. **None of the proposals address the `CARGO_PKG_VERSION` drift problem.** After a `git pull` + `cargo build --release`, the new binary in `target/release/` has a new `CARGO_PKG_VERSION` baked in at compile time. The *currently running* binary still reports the old version. `openassistant status` run before and after the update (without restarting) will report the same version. There is no mechanism to tell the user "you are running version X, the new binary is version Y — please restart." This should be addressed in the update feature.


---

# Per-Feature Research Briefs


## Brief: update

I now have all the information needed to produce a concrete brief. Here it is:

---

## Wire the Real `update` CLI Command — Research Brief

### 1. What Already Exists in `src/core/self_update.rs`

**Functional:**
- `SelfUpdater::new(workspace_dir)` — takes a filesystem path (the git checkout root).
- `check_update()` — async; GETs `https://crates.io/api/v1/crates/open-assistant`, parses `json["crate"]["newest_version"]`, returns `Ok(Some(version_string))` when a newer version exists. Uses `reqwest::Client` (already in `Cargo.toml` at 0.12). Sets a proper `User-Agent` header.
- `update_from_source()` — async; checks `permission != Deny`, runs `git pull --rebase` then `cargo build --release` in `workspace_dir` via `tokio::task::spawn_blocking` + `std::process::Command`. Returns `Ok(true)` if a build was triggered, `Ok(false)` if already up to date.
- `Permission` enum: `Allow | Ask | Deny` (default is `Ask`).

**Missing / Gaps:**
- No dirty-tree check (`git status --porcelain`) before `git pull`. A dirty tree causes `git pull --rebase` to abort; the error is captured and returned as an `anyhow::Err`, but the message won't be user-friendly.
- No `--check` vs `--apply` distinction in the struct or call site.
- crates.io version check is wrong for a source-installed crate: the `open-assistant` crate is **not published to crates.io** (version is `0.1.0`, the repository URL is a placeholder GitHub org that does not exist). In practice `check_update()` will always return `Ok(None)` because the registry request will 404 or return no `newest_version` field. The actual available-version signal for a source install is **git** (`git fetch && git rev-parse HEAD..origin/HEAD`).
- No `workspace_dir` in config — the CLI must supply its own detection (see below).
- `update_from_source` builds `--release` unconditionally. That is fine, but the user may have installed a debug build; this is acceptable.
- No stdin confirmation prompt — the function proceeds if `permission != Deny`, but `permission` is always `Ask` (no path to set `Allow` from the CLI).
- The `self_update` arm in `core/agent.rs` (line 554) returns a static instruction string and does **not** call `SelfUpdater` at all.

**Already in `main.rs` (line 118–121):**
```rust
Commands::Update => {
    println!("Use 'cargo update && cargo build --release' to update from source.");
    println!("Or run 'openassistant onboard' to reconfigure.");
}
```
This is the pure-placeholder that needs replacing.

---

### 2. Best-Practice Rust Self-Update Strategies

| Strategy | When to use | Fits this project? |
|---|---|---|
| `self_update` crate (GitHub releases/S3) | Distributed binaries; replaces the running executable | No — no published release artifacts |
| `cargo-dist` + updaters | Projects with CI-built release binaries | No — same reason |
| git pull + cargo build | Source installs / developer tools where user has the repo | **Yes** — this is exactly how openAssistant is installed today |
| Tauri `tauri-plugin-updater` | Tauri 2.x desktop apps with a published update endpoint | Later item — see §4 |

**Verdict:** Git pull + cargo build is the correct approach for the current distribution model. The `self_update` crate is unnecessary and adds complexity. `SelfUpdater::update_from_source()` already implements this; it just needs to be called.

**Workspace dir detection:** The binary does not know its source checkout path from config. The idiomatic approach is to try, in order:
1. `OPENASSISTANT_SRC` env var (escape hatch for non-standard installs).
2. `std::env::current_exe()` → walk upward to find a directory containing `Cargo.toml` with `name = "open-assistant"`.
3. Fallback: fail with a helpful message telling the user to set `OPENASSISTANT_SRC`.

This keeps the logic out of `SelfUpdater` (which already accepts the path) and inside the CLI match arm.

---

### 3. How `update` Should Behave: CLI vs Desktop

**CLI (now):**

Add two flags to `Commands::Update`:
```rust
Update {
    /// Only check, do not apply
    #[arg(long)]
    check: bool,
    /// Skip confirmation prompt
    #[arg(long)]
    yes: bool,
}
```

Flow:
1. Detect workspace dir (env var → exe-parent walk → error).
2. Print current version (`env!("CARGO_PKG_VERSION")`).
3. Run `git fetch` in the workspace dir, then check `git rev-parse HEAD..origin/HEAD` — if empty, print "Already up to date" and exit.
4. If `--check`: print the pending commits summary (`git log HEAD..origin/HEAD --oneline`) and exit 0.
5. Otherwise: print the pending changes, then (unless `--yes`) print `"Apply update? [y/N] "` and read stdin.
6. On confirmation: check `git status --porcelain`; if non-empty, abort with "Working tree is dirty — commit or stash changes first."
7. Call `updater.update_from_source()`. Stream stdout/stderr live (current implementation captures at end — acceptable for MVP but noted).
8. On success: print "Update complete. Restart openassistant to use the new binary."

**Desktop (later):** `tauri-plugin-updater` is not in `src-tauri/Cargo.toml` and there is no update endpoint. The CAPABILITY HONESTY TABLE rule in `lib.rs` means it must **not** get a UI affordance until wired for real. Note it as a follow-up: requires publishing update bundles to a versioned endpoint (GitHub Releases or a CDN), adding `tauri-plugin-updater = "2"` + capability, and wiring a `check_update` Tauri command. That is a separate shipping milestone.

---

### 4. Minimal Viable Wiring (Ships Now)

**Files to touch:**

1. **`src/main.rs`** — Replace the `Update` variant and its match arm.
   - Change `Update` to `Update { check: bool, yes: bool }` (lines 34, 118–121).
   - Add the 8-step flow described above (~50 lines of new match arm code).
   - Import: `use open_assistant::core::self_update::SelfUpdater;` (already exported via `src/core/mod.rs` line 7, and `src/lib.rs` `pub mod core`).

2. **`src/core/self_update.rs`** — Add two helpers used by the new CLI arm:
   - `check_pending() -> Result<Vec<String>>`: runs `git fetch` then `git log HEAD..origin/HEAD --oneline`, returns the lines.
   - `is_dirty() -> Result<bool>`: runs `git status --porcelain`, returns `true` if non-empty output.
   Both follow the existing `run_cmd` pattern already in the file.

3. **`src/core/agent.rs` line 554** — Optional but low-cost: change the `self_update` tool response to say "Run `openassistant update --check` to see available updates, or `openassistant update` to apply." (No structural change, one string edit.)

**No new dependencies needed.** `reqwest` is already present (used by `check_update`); the dirty-tree and pending-commit checks use `std::process::Command` already used by `run_cmd`. Stdin reading uses `std::io::stdin()`.

---

### 5. Risks

- **crates.io check is permanently broken** for `check_update()` since the crate is not published. The MVP wiring should not call `check_update()` at all — use `check_pending()` (git-based) instead. Keep `check_update()` in the struct but do not call it from the CLI arm. Remove or document the crates.io path as a dead end until/unless the crate is published.
- **`git pull --rebase` replaces the running binary on Windows while it is running.** On Windows you cannot replace an open `.exe`. The build step produces a new `.exe` in `target/release/`; the user must restart. This is unavoidable with the source-build approach and should be stated clearly in the output message. (On Linux/macOS the binary is replaced in place and a new process sees it.)
- **Long build time with no output.** `cargo build --release` takes 1–3 minutes and the current `run_cmd` implementation captures output only at completion (`output()` not `spawn()`). For MVP this is acceptable if you print "Building… (this may take a few minutes)" before calling. A follow-up can stream stdout via `tokio::process::Command` + `lines()`.
- **`spawn_blocking` wraps a blocking `Command::output()`** — this is correct for `tokio::task::spawn_blocking` but the `ExitStatus::default()` fallback on `.await.unwrap_or(...)` silently swallows join errors. Low risk for CLI use but worth documenting.

---

### 6. Design Decisions That Need a Verdict

**Decision A: Workspace dir source of truth.**
Should the detected workspace dir be persisted to `config.yaml` (e.g., `general.workspace_dir`) so the user can override it without an env var? Or is the exe-parent walk + env-var-override sufficient?

- *Recommendation:* Start with exe-parent walk + `OPENASSISTANT_SRC` env var. Persisting to config adds a write on first `update` run and a new config key; the env var is the conventional escape hatch for developer tools. Revisit if users report install layouts that break the walk.

**Decision B: `--check` exit code contract.**
Should `update --check` exit 1 when updates are available (scriptable), 0 always (informational only), or 0 when up-to-date and 1 when updates exist?

- *Recommendation:* Exit 0 when up to date, exit 1 when updates are pending — this matches `git fetch` + `git status` tooling conventions and lets CI scripts use `openassistant update --check || notify_team`. Add a `--quiet` flag later if needed.


## Brief: workflow

I now have a comprehensive picture of the codebase. Here is the research brief:

---

## Workflow Real Execution — Research Brief

### 1. What the Current Code Actually Does

**File:** `src/core/workflows.rs`

`WorkflowEngine::execute()` (lines 95–202) already has a correct dependency-DAG loop:

- The `while !remaining.is_empty()` loop at line 121 identifies steps whose `depends_on` predecessors are all present in `completed: HashMap<String, StepResult>`.
- Ready steps are fanned out with `tokio::spawn` in parallel.
- Results are joined, inserted into `completed`, and `remaining` is trimmed — correct topological execution.

**The only fake part** is lines 141–161 inside each `tokio::spawn` closure:

```rust
let output = format!(
    "Step '{}' completed: {}",
    step_clone.id,
    step_clone.description
);
StepResult { ..., output: Some(output), ... }
```

This closure has no access to the LLM or any agent infrastructure — it runs in a plain `tokio::spawn` with no `Arc`-shared state.

`fan_out()` (line 205) and `fan_in()` (line 228) are similarly hollow; they are not called by `execute()`.

`list_runs()` (line 236) always returns `Vec::new()` — a placeholder acknowledged by the comment about Arc/Mutex.

---

### 2. What "Real Execution" Needs

#### 2a. The Key Structural Problem: `tokio::spawn` + LLM Call

`call_llm()` in `agent.rs` (line 178) loads config from disk via `config::load().await?` and fires a `reqwest` HTTP call. Both are `async` and `Send`. This means a real LLM call **can** run inside `tokio::spawn` — no fundamental blocker.

However, `Agent::process()` requires `&mut FullContext` and `&mut Session`. These are `!Send` when held across an await through an `Arc<Mutex<FullContext>>`, but if each step gets its **own fresh** `FullContext` and `Session`, there is no sharing problem.

The decision the team needs to make (see section 5) is which unit of work gets called per step.

#### 2b. Passing Prior-Step Output as Context to Dependent Steps

`StepResult` already has `output: Option<String>`. After `handle.await` the `completed` map holds all finished results. Before spawning a ready step, the caller must:

1. Look up the step's `depends_on` list.
2. Collect their `StepResult::output` strings from `completed`.
3. Embed that context into the prompt sent to the LLM for this step.

Concretely: in `execute()`, change the spawn closure from receiving only `step_clone` to also receiving `prior_outputs: Vec<(String, String)>` (step_id → output text). That vector is cheaply cloneable (`String` types only). The step's LLM prompt becomes:

```
You are executing workflow step '{step.id}': {step.description}

Prior step outputs:
[step_a] <output>
[step_b] <output>

Complete this step and produce your result.
```

No new types are needed; the existing `StepResult.output` field is the wire.

#### 2c. Persisting WorkflowRun State

Currently `active_runs: HashMap<String, Arc<Mutex<WorkflowRun>>>` is held inside `WorkflowEngine`, which is constructed fresh in `main.rs`'s `Commands::Workflow` arm (line 272) and dropped at process exit. `list_runs()` deliberately returns `Vec::new()` because it cannot return `&WorkflowRun` from behind `Arc<Mutex>` without holding a guard.

**The existing SQLite pattern** at `src/memory/store.rs` is fully ready: `rusqlite` with `bundled` feature, real `Connection::open`, FTS5 triggers. A `workflow_runs` table can be added to the same `memory.db` or a dedicated `workflows.db`. The schema maps directly onto `WorkflowRun` and `StepResult` fields (all `String`, `DateTime<Utc>` as RFC-3339, `WorkflowStatus` as TEXT).

**Minimal persistence design:**

```sql
CREATE TABLE IF NOT EXISTS workflow_runs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    total_steps INTEGER NOT NULL,
    completed_steps INTEGER NOT NULL,
    start_time TEXT NOT NULL,
    end_time TEXT
);

CREATE TABLE IF NOT EXISTS workflow_step_results (
    run_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    status TEXT NOT NULL,
    output TEXT,
    error TEXT,
    start_time TEXT NOT NULL,
    end_time TEXT,
    PRIMARY KEY (run_id, step_id),
    FOREIGN KEY (run_id) REFERENCES workflow_runs(id)
);
```

`WorkflowEngine` gains a `rusqlite::Connection` field (or an `Arc<Mutex<Connection>>`). After every step completion and at run end, the engine writes a row. `list_runs()` queries the table instead of the in-memory map.

**Blocker:** `rusqlite::Connection` is not `Send`, so it cannot live inside the `tokio::spawn` closures directly. The correct pattern (already used by `MemoryStore`) is to keep the `Connection` on the caller side and write results **after** the `handle.await` loop, not inside the spawned task. This matches the current code structure exactly — the join loop at line 166 runs on the engine's thread and is the right place to upsert each `StepResult` to SQLite.

---

### 3. The Core Design Decision: Full `Agent::process()` vs. Lighter `call_llm()`

This is the most consequential choice.

**Option A — Full `Agent::process()` per step**

Each step gets a new `Agent`, a new `FullContext`, and a new `Session`. The agent loop writes daily notes, builds the full system prompt (persona + memory), and can dispatch tool calls ([TOOL:...] regex) inside a single turn. Tool dispatch means a step can read files, run bash, search — real multi-tool work.

Costs:
- `Agent::process()` writes to disk (daily notes via `MemoryWorkspace::append_daily`) on every step — concurrent parallel steps will race on `memory/YYYY-MM-DD.md`. Need a `Mutex<MemoryWorkspace>` shared across steps or per-step isolation (different data_dir prefixes).
- Each step's `FullContext` starts blank (no user model from prior sessions). For workflow steps this is actually desirable — each step is a self-contained task agent.
- The `FullContext` and `Session` are not `Send`; must be constructed inside the `tokio::spawn` closure, not before it.
- `Agent::process()` is `async` and calls `config::load()` (disk I/O) at every step. With many parallel steps this is redundant — load config once and pass as `Arc<Config>`.

**Option B — Lighter single `call_llm()` per step**

Call the HTTP endpoint directly: build a minimal system prompt (step description + prior outputs), POST to `{api_base}/chat/completions`, get the text back. No tool dispatch, no memory writes, no persona overhead.

Costs:
- No tool use within a step. Steps are "think and write" only. For the built-in `code-review` workflow (which has `explore`, `review-security`, `review-quality`, `summarize` steps), the explore step needs file read tools to be useful.
- `call_llm()` is currently a private method on `Agent`. It would need to be extracted to a free function `core::agent::call_llm_raw(config: &ModelConfig, messages: &[serde_json::Value]) -> Result<String>`, or `WorkflowEngine` would need to borrow an `&Agent`.

**Recommendation for MVP:** Option B for the initial cut — it unblocks real output immediately, avoids the MemoryWorkspace concurrency hazard, and keeps the spawn closures `Send` without fighting the borrow checker. Upgrade to Option A (with per-step tool dispatch) in a follow-up once the memory-write race is addressed (e.g., channel-based write serialization).

---

### 4. Realistic MVP Scope

**What changes and where:**

| # | What | File | Lines to touch |
|---|------|------|----------------|
| 1 | Extract `call_llm_raw(config, messages) -> Result<String>` as a public free function or `pub(crate)` method | `src/core/agent.rs` | Lines 178–223 — pull into a standalone `async fn` that takes `&ModelConfig` + messages slice |
| 2 | Add `WorkflowStore` (SQLite, same pattern as `MemoryStore`) | `src/core/workflows.rs` (new sub-module or inline) | New struct with `open(path)`, `upsert_run`, `upsert_step`, `list_runs` |
| 3 | Change `WorkflowEngine` to hold `config: Arc<Config>` and `db: WorkflowStore` | `src/core/workflows.rs` line 70–73 | New fields; `WorkflowEngine::new()` → `WorkflowEngine::new_with_config(config, db_path)` |
| 4 | Replace fake spawn closure with real LLM call | `src/core/workflows.rs` lines 139–162 | Pass `Arc<Config>` + `Arc<prior_outputs: Vec<(String,String)>>` into spawn; build a 2-message prompt; call `call_llm_raw` |
| 5 | Write step results to SQLite after join loop | `src/core/workflows.rs` lines 166–175 | After `completed.insert(...)`, call `self.db.upsert_step(...)` |
| 6 | Fix `list_runs()` to query SQLite | `src/core/workflows.rs` line 236 | Return `Vec<WorkflowRunSummary>` from DB query |
| 7 | Wire `WorkflowEngine::new_with_config` in CLI | `src/main.rs` lines 271–279 | `config::load().await?` then pass to engine constructor |
| 8 | Add `WorkflowEngine::get_run_status(run_id)` | `src/core/workflows.rs` | Query DB for run + step rows; return formatted report |

**Non-Goals for MVP:**
- Tool dispatch inside steps (no `bash`/`read`/`glob` within a workflow step) — Option A is a follow-up.
- Multi-turn loops within a step (one LLM call per step only).
- Desktop surface for workflow status — desktop explicitly does not expose stubs as working features per CLAUDE.md's CAPABILITY HONESTY TABLE.
- `fan_out()` / `fan_in()` public methods — they remain unused stubs; `execute()` is the real entry point.
- Streaming step output — post-MVP.

---

### 5. Risks and Decisions Needed

**Decision 1 — Option A vs. Option B (full agent vs. raw LLM call per step)**
Verdict needed before implementation. The recommendation is Option B for MVP with a clear upgrade path: once `MemoryWorkspace` writes are serialized (e.g., a `tokio::sync::Mutex`-wrapped writer struct), steps can opt in to full `Agent::process()` via a `WorkflowStep.agent_mode: bool` field.

**Decision 2 — Single `workflows.db` or add tables to `memory.db`**
`MemoryStore::open()` takes a path string; opening `memory.db` a second time from `WorkflowEngine` on the same thread is safe with SQLite WAL mode, but WAL is not currently enabled. Safer MVP choice: open a dedicated `~/.openassistant/workflows.db`. Switch to the shared DB later if desired.

**Risk: `config::load()` called N times for N parallel steps**
Each step closure currently must call `config::load().await?` because `call_llm()` does so internally. With 4 parallel steps this means 4 concurrent YAML reads — harmless but wasteful. Fix: load config once in `execute()`, wrap in `Arc<Config>`, pass into closures. Requires extracting `call_llm_raw(config: &ModelConfig, messages: &[serde_json::Value])` first (change #1 above).

**Risk: the built-in `code-review` workflow is meaningless without tool use**
The `explore` step's description says "find changed files" — without file-read tools the LLM will hallucinate or produce generic text. Either (a) accept this as a demo limitation and document it, or (b) replace the built-in with a simpler "summarize + critique" workflow that has no tool dependency for the MVP.

**Risk: `tokio::spawn` requires `'static` + `Send`**
`Arc<Config>` is `Send + Sync`. `prior_outputs: Vec<(String, String)>` is `Send`. The closures will compile cleanly. `FullContext` and `Session` must be constructed inside the spawn (not borrowed from the outer scope) — verified feasible for Option B since neither is needed in the lightweight path.


## Brief: checkpoint

I now have everything I need. Here is the research brief.

---

## Checkpoint Persistence: Research Brief

### 1. Current State (Verified)

**`src/core/checkpoint.rs`** — `CheckpointStore` is a plain `Vec<Checkpoint>` with `max_checkpoints: 50` (line 27). The `Checkpoint` struct carries `id`, `session_id`, `timestamp`, `description`, `file_hashes: HashMap<String,String>`, and `file_snapshots: HashMap<String,String>` (full file text). `file_snapshots` is the largest field; it holds all snapshotted content in-memory. Everything is lost on restart.

**`src/main.rs` lines 281-297** — `Commands::Checkpoint` creates a *fresh* `CheckpointStore::new()` on every invocation, then immediately tries to list from it. Only `--action list` is handled; `create` and `restore` are dead code in the `_` arm that prints usage. `id` is overloaded to mean both checkpoint-id (for restore) and session-id (for list). Workspace directory is set to `config.general.data_dir` but never passed into the store.

**`sha2` 0.10** is already in `Cargo.toml` (line 67). `rusqlite 0.32` with `bundled` and `chrono` features is already there (line 46). No new deps are needed.

### 2. SQLite vs. JSON-file Decision

**Verdict: SQLite, reusing `memory.db` via a new table.**

Rationale:

- The codebase already opens `memory.db` in `MemoryStore::open_default()` and `open()`. The `rusqlite::Connection` pattern (sync, `execute_batch`, `params![]`) is established in 196 lines of `src/memory/store.rs`. No new file format or parser.
- `rusqlite` with the `bundled` feature means no external library. The connection is already `Send`-safe across the tokio runtime boundary (the memory store wraps a sync `Connection` and callers use `tokio::task::spawn_blocking` or just call from a non-async context).
- JSON files under `data_dir/checkpoints/` are simpler to inspect but add a second I/O pattern (directory scanning, file naming, manual pruning), and `file_snapshots` content could be multiple MB per checkpoint — you'd be storing those blobs as standalone files. SQLite's `BLOB`/`TEXT` column with a single `DELETE` for pruning is cleaner.
- The one argument *for* JSON: it is trivially human-readable and `git diff`-friendly. That benefit is marginal for an assistant tool that already uses a DB for memory.

**Reuse `memory.db`, add two tables (`checkpoints`, `checkpoint_files`).** Avoids a second DB open/init on every invocation and keeps the single-file footprint.

### 3. Schema Design

```sql
CREATE TABLE IF NOT EXISTS checkpoints (
    id          TEXT PRIMARY KEY,          -- "cp_<8-char-uuid>"
    session_id  TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,             -- RFC 3339, chrono feature active
    file_count  INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_cp_session ON checkpoints(session_id, created_at);

CREATE TABLE IF NOT EXISTS checkpoint_files (
    checkpoint_id TEXT NOT NULL REFERENCES checkpoints(id) ON DELETE CASCADE,
    file_path     TEXT NOT NULL,
    file_hash     TEXT NOT NULL,           -- SHA-256 hex
    content       TEXT NOT NULL,           -- full file text
    PRIMARY KEY (checkpoint_id, file_path)
);
```

`ON DELETE CASCADE` means pruning is a single `DELETE FROM checkpoints WHERE ...` and the file rows follow automatically. `file_count` is a denormalized convenience for `format_checkpoints()` — avoids a join when listing.

Enable cascade by running `PRAGMA foreign_keys = ON` on each connection open (SQLite disables it by default).

### 4. Proposed `CheckpointStore` Rewrite

Replace the `Vec<Checkpoint>` with a `rusqlite::Connection`:

```rust
pub struct CheckpointStore {
    conn: Connection,
    max_checkpoints: usize,
}

impl CheckpointStore {
    pub fn open(db_path: &str) -> Result<Self> { ... }
    pub fn open_default() -> Result<Self> { /* mirror MemoryStore::open_default, same data_dir */ }
    // existing method signatures unchanged; now do INSERT/SELECT/DELETE
}
```

**Key implementation notes:**

- `create_checkpoint`: INSERT into `checkpoints` then batch-INSERT into `checkpoint_files`, then call `prune_old_checkpoints`. All in a transaction (`conn.execute("BEGIN", [])` / `COMMIT`) so a partial write cannot leave a dangling checkpoint.
- `restore_checkpoint`: SELECT `content` from `checkpoint_files WHERE checkpoint_id = ?`, write to disk (same logic as today, lines 88-99).
- `list_checkpoints`: SELECT `checkpoints` WHERE `session_id = ?` ORDER BY `created_at ASC`; return `Vec<Checkpoint>` with `file_snapshots` left empty (lazy load) to avoid pulling MB of file content on every list. Populate `file_hashes` from `checkpoint_files` only if needed.
- `prune_old_checkpoints`: `DELETE FROM checkpoints WHERE session_id = ? AND id NOT IN (SELECT id FROM checkpoints WHERE session_id = ? ORDER BY created_at DESC LIMIT ?)` — cascade handles `checkpoint_files`.
- `format_checkpoints`: unchanged logic, drives off `file_count` column.

Because `Connection` is `!Send`, callers in `main.rs` (already `#[tokio::main]`) must either use `tokio::task::spawn_blocking` or open the store inside a sync function. Given that `main.rs` checkpoint handling is simple sequential CLI logic (no concurrent tasks), the simplest path is to call `CheckpointStore::open_default()` as a sync call (make it `fn`, not `async fn`, matching `MemoryStore::open`'s `async fn` which only needs `tokio::fs::create_dir_all`). Alternatively, mirror the pattern exactly and keep it `async fn open_default`.

### 5. Wiring `main.rs` Commands::Checkpoint

The `id` argument is currently overloaded. Add a `--session` flag and keep `--id` for the checkpoint id:

```rust
Checkpoint {
    #[arg(long)] action: Option<String>,
    #[arg(long)] id: Option<String>,        // checkpoint id (for restore/describe)
    #[arg(long)] session: Option<String>,   // session id (for list/create)
    #[arg(long)] workspace: Option<String>, // directory for create/restore (defaults to cwd)
    #[arg(long)] files: Vec<String>,        // file paths for create
    #[arg(long)] description: Option<String>,
},
```

Wire all four actions:

| action | args used | what it does |
|---|---|---|
| `list` | `--session` | `store.list_checkpoints(session)` → `format_checkpoints` |
| `create` | `--session`, `--workspace`, `--files`, `--description` | `store.create_checkpoint(...)` |
| `restore` | `--id`, `--workspace` | `store.restore_checkpoint(id, workspace)` |
| `delete` | `--id` | `DELETE FROM checkpoints WHERE id = ?` (new method) |

`workspace` defaults to current directory (`std::env::current_dir()`). `session` defaults to a stable ID derived from the workspace path (hash or path string) so a user doesn't have to track a UUID.

### 6. Files to Touch

| File | Change |
|---|---|
| `src/core/checkpoint.rs` | Replace `Vec<Checkpoint>` with rusqlite `Connection`; add `open()` / `open_default()`; implement schema init, CRUD, pruning; keep method signatures source-compatible |
| `src/main.rs` | Add `--session`, `--workspace`, `--files`, `--description` to `Checkpoint` variant; add `create`, `restore`, `delete` match arms; call `CheckpointStore::open_default()?` instead of `CheckpointStore::new()` |
| `src/memory/store.rs` | No change required (reusing the same `memory.db` file means CheckpointStore's `init()` can call `ATTACH` or simply open the same path) |

No new crate dependencies.

### 7. Pruning & `max_checkpoints`

Current default is 50 per session (line 27). Persist this as a compile-time const or make it config-driven via a future `config.general.max_checkpoints`. For MVP, keep the `max_checkpoints: usize` field on `CheckpointStore` defaulting to 50, just enforce it with a SQL DELETE instead of `Vec::retain`.

### 8. Design Decisions Needing a Verdict

**Decision A — Same `memory.db` vs. separate `checkpoints.db`:**
Recommendation above is `memory.db`. Counter-argument: checkpoint files can be large (many MB of source text); mixing them with the memory FTS store risks page-cache pressure and makes `VACUUM` slower. If checkpoints typically snapshot large files, a separate `checkpoints.db` is safer and lets you `PRAGMA journal_mode=WAL` independently. Verdict needed before implementation.

**Decision B — Lazy vs. eager `file_snapshots` on `list_checkpoints`:**
`list_checkpoints` currently returns `&Checkpoint` with full `file_snapshots`. With SQLite, loading all file content on every list is expensive for deeply-used sessions. The natural fix is to return a lightweight `CheckpointMeta` (id, session_id, timestamp, description, file_count) from `list_checkpoints` and a separate `load_checkpoint(id)` that fetches file content. This breaks the current public API signature. Verdict needed on whether callers (agent loop in `agent.rs` calls `latest_checkpoint`) can tolerate the split API.

### 9. MVP Cut

**In scope now:**
- `CheckpointStore::open_default()` opening `memory.db`, creating the two tables on first run.
- `create_checkpoint` and `restore_checkpoint` backed by SQLite.
- `prune_old_checkpoints` via DELETE with LIMIT subquery.
- `main.rs` wiring for `list`, `create`, `restore`.

**Out of scope / later:**
- Config-driven `max_checkpoints` setting.
- `delete` action (nice to have but not critical for correctness).
- Agent loop auto-checkpointing (currently `agent.rs` does not call `CheckpointStore` at all — that wiring is a separate feature).
- Compression of `content` blobs (e.g., zstd) for large workspace snapshots.
- Cross-session `list --all`.

### 10. Risk

The only real risk is the `rusqlite::Connection` is `!Send`. The existing `MemoryStore` works around this by being opened and dropped within a single sync context or by the pattern of `spawn_blocking`. In `main.rs` the checkpoint command is purely sequential and non-concurrent, so opening `CheckpointStore` synchronously in the `Commands::Checkpoint` match arm is safe. The test suite uses `tempfile::tempdir()` already (line 173 of `checkpoint.rs`), so existing tests continue to work by passing a temp DB path.


## Brief: multimodel

I now have a complete picture of the codebase. Let me compile the full brief.

---

# Multi-Provider / Multi-Modality Model Routing — Design Brief

## 1. Current State (grounded in source)

### Config layer (`src/config/mod.rs:56-72`)

```
pub struct ModelConfig {
    pub provider: String,   // "openrouter"
    pub model: String,      // "openrouter/owl-alpha"
    pub api_key: String,
    pub api_base: String,   // "https://openrouter.ai/api/v1"
}
```

One flat `ModelConfig`. No `#[serde(default)]` on the struct itself — fields are required; adding new keys must use `#[serde(default)]` or the whole struct to stay backward-compatible.

### Agent construction sites (all grab `config.model.model` and nothing else)

| File | Line / pattern |
|---|---|
| `src/ui/chat.rs:18` | `Agent::new(&config.model.model)` |
| `src-tauri/src/lib.rs:38` | `Agent::new(cfg.model.model)` |
| `src-tauri/src/commands/settings.rs:186` | `Agent::new(cfg.model.model.clone())` |
| TUI / Web (`src/ui/mod.rs:34`) | hardcoded fallback `"openrouter/owl-alpha"` |

### `call_llm` (`src/core/agent.rs:178-223`)

Calls `config::load().await?` fresh on **every turn**. Picks up `config.model.api_base` and `config.model.api_key` unconditionally. The `body["model"]` is `self.model` (string stored on Agent at construction time), not re-read from config. So if config changes mid-session the key/base update, but the model string does not, unless `rebuild_agent_if_model_changed` fires (desktop only, `settings.rs:182`).

### Vision (`src/tools/vision.rs`)

Shells out to the `gemini` CLI binary with `Command::new("gemini")`. No API call. Uses `config.vision.gemini_path` (read nowhere in the tool — the tool hardcodes `"gemini"`; `config.vision` exists in config but is not injected into `vision::execute`). Completely decoupled from `call_llm`.

### `config::set()` allowlist (`src/config/mod.rs:176-190`)

Only handles: `model.provider`, `model.model`, `model.api_key`, `gateway.*`, `security.*`. `model.api_base` is **not** in the allowlist; you can set the API base only via direct YAML edit or the desktop `save_config` command.

---

## 2. Proposed Config Schema (Backward-Compatible)

### 2a. New structures

```rust
// src/config/mod.rs — additions

/// One named provider entry. api_base must be OpenAI-compatible for text/vision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderEntry {
    pub name: String,        // e.g. "openrouter", "openai", "anthropic-proxy"
    pub api_base: String,    // e.g. "https://openrouter.ai/api/v1"
    pub api_key: String,
}

impl Default for ProviderEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            api_base: String::new(),
            api_key: String::new(),
        }
    }
}

/// Per-modality routing: which provider + which model string to use.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModalityRoute {
    pub provider: String,   // must match a ProviderEntry.name
    pub model: String,
}

impl Default for ModalityRoute {
    fn default() -> Self { Self { provider: String::new(), model: String::new() } }
}

/// Routing table for all modalities.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RoutingConfig {
    pub text: ModalityRoute,
    pub vision: ModalityRoute,
    pub image_gen: ModalityRoute,   // stub; no endpoint wired yet
    pub video: ModalityRoute,       // stub; no endpoint wired yet
}
```

### 2b. Extend `Config` — backward compatible

```rust
pub struct Config {
    pub general: GeneralConfig,
    pub model: ModelConfig,       // kept as-is; becomes the fallback provider
    pub gateway: GatewayConfig,
    pub memory: MemoryConfig,
    pub skills: SkillsConfig,
    pub security: SecurityConfig,
    pub vision: VisionConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    // NEW — both get #[serde(default)] so old YAML files load cleanly:
    #[serde(default)]
    pub providers: Vec<ProviderEntry>,
    #[serde(default)]
    pub routing: RoutingConfig,
}
```

### 2c. Fallback resolution function

```rust
// src/config/mod.rs — new public fn

/// Resolve provider creds for a given modality.
/// Falls back to config.model if no providers list or no matching route.
pub fn resolve_provider<'a>(
    config: &'a Config,
    modality: &str,   // "text" | "vision" | "image_gen" | "video"
) -> (&'a str, &'a str, &'a str) {   // (api_base, api_key, model)
    let route = match modality {
        "vision"    => &config.routing.vision,
        "image_gen" => &config.routing.image_gen,
        "video"     => &config.routing.video,
        _           => &config.routing.text,
    };

    if !route.provider.is_empty() {
        if let Some(entry) = config.providers.iter().find(|p| p.name == route.provider) {
            return (&entry.api_base, &entry.api_key, &route.model);
        }
    }
    // Fallback: legacy ModelConfig
    (&config.model.api_base, &config.model.api_key, &config.model.model)
}
```

---

## 3. Refactoring `call_llm` for Per-Modality Dispatch

### 3a. Add a `modality` parameter to `call_llm`

Current signature: `async fn call_llm(&self, messages: &[serde_json::Value]) -> Result<String>`

New signature:
```rust
async fn call_llm(
    &self,
    messages: &[serde_json::Value],
    modality: &str,   // "text" | "vision"
) -> Result<String>
```

All internal callers pass `"text"`. The `vision` tool path (new API-based branch, see §4) calls with `"vision"`.

Inside `call_llm` replace the two hard-coded picks:

```rust
// BEFORE
let body = serde_json::json!({ "model": self.model, ... });
// ... config.model.api_key, config.model.api_base

// AFTER
let config = crate::config::load().await?;
let (api_base, api_key, model_name) = crate::config::resolve_provider(&config, modality);

if api_key.trim().is_empty() { anyhow::bail!(...); }

let body = serde_json::json!({ "model": model_name, ... });
let resp = client.post(format!("{}/chat/completions", api_base))
    .header("Authorization", format!("Bearer {}", api_key))
    ...
```

`self.model` is then only used as a fallback when `routing.text` is empty (handled inside `resolve_provider`), and `rebuild_agent_if_model_changed` in the desktop settings command remains valid as a display-only hint — real routing comes from config.

### 3b. `Agent` struct change

`Agent.model: String` can stay for backward compat and display (status panels read it). `call_llm` ignores it when a routing entry is present.

---

## 4. Vision: API-Based Routing vs. Gemini CLI

### Current state

`src/tools/vision.rs` does `Command::new("gemini")` — no HTTP, no API key. `config.vision.gemini_path` is in the YAML but the tool ignores it (hardcodes `"gemini"`).

### MVP design decision (see §6 Decision B)

Two branches, selected by `config.vision.provider`:

| `config.vision.provider` | Behavior |
|---|---|
| `"gemini-cli"` (default) | existing `Command::new(&config.vision.gemini_path)` — keeps today's behavior |
| `"api"` | new branch: send the image as a base64 `image_url` content part via the vision routing entry |

**API-based vision** uses the standard OpenAI `chat/completions` multipart content format:
```json
{
  "role": "user",
  "content": [
    {"type": "text", "text": "<question>"},
    {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<b64>"}}
  ]
}
```

This is supported by OpenRouter (routes to GPT-4o, Claude 3 Vision, etc.), OpenAI directly, and Anthropic-compatible proxies. The `base64` crate is already in `Cargo.toml:69`.

New helper in `src/tools/vision.rs`:
```rust
pub async fn execute_api(
    args: &serde_json::Value,
    config: &crate::config::Config,
) -> Result<ToolResult>
```

Which calls `call_llm` with `modality = "vision"` by constructing the image-URL message. For now it reads the file from disk and base64-encodes it; URL inputs are a later enhancement.

The `vision` arm in `handle_tool_calls` (`agent.rs:327`) becomes:
```rust
"vision" => {
    let result = match config.vision.provider.as_str() {
        "api" => crate::tools::vision::execute_api(&tool_call.arguments, &config).await?,
        _     => crate::tools::vision::execute(&tool_call.arguments).await?,
    };
    Ok(format!("{}\n\nVision result:\n{}", response, result.output))
}
```

Config is already loaded earlier in `handle_tool_calls`' context because `call_llm` does it; pass it through or load once at top of the function.

---

## 5. Image Generation and Video (Stub-but-Structured)

These are **not wired** in MVP. The config schema (`routing.image_gen`, `routing.video`) is added and parses cleanly. A new `src/tools/image_gen.rs` stub:

```rust
pub async fn execute(args: &serde_json::Value, config: &Config) -> Result<ToolResult> {
    let route = &config.routing.image_gen;
    if route.provider.is_empty() || route.model.is_empty() {
        return Ok(ToolResult {
            success: false,
            output: String::new(),
            error: Some("image_gen routing not configured (set routing.image_gen in config.yaml)".into()),
        });
    }
    // POST to {api_base}/images/generations — OpenAI DALL-E / fal.ai / Stability AI
    // STUB: full implementation pending
    Ok(ToolResult { success: false, output: String::new(),
        error: Some("image_gen: not yet implemented".into()) })
}
```

This registers in `ToolRegistry::execute` and `default_tools()` but returns an honest error. No fake "In a full implementation" text.

---

## 6. Files and Functions to Touch

| Priority | File | Change |
|---|---|---|
| P0 | `src/config/mod.rs` | Add `ProviderEntry`, `ModalityRoute`, `RoutingConfig`; extend `Config` with `#[serde(default)] providers` + `routing`; add `resolve_provider()` |
| P0 | `src/core/agent.rs:178` | `call_llm` gains `modality: &str` param; uses `resolve_provider` instead of `config.model.*` directly |
| P0 | `src/core/agent.rs:94` | `call_llm(&messages)` → `call_llm(&messages, "text")` at every call site |
| P1 | `src/tools/vision.rs` | Fix hardcoded `"gemini"` to use `config.vision.gemini_path`; add `execute_api()` branch |
| P1 | `src/core/agent.rs:327` | `vision` match arm: branch on `config.vision.provider` |
| P1 | `src/config/mod.rs:176` | Extend `set()` allowlist for `model.api_base`, `routing.*`, `providers.*` (or document not settable via CLI) |
| P2 | `src/tools/image_gen.rs` | New stub module |
| P2 | `src/tools/mod.rs` | Register `image_gen` |
| P2 | `src/core/agent.rs` | Add `image_gen` to `default_tools()` + `handle_tool_calls` |
| P2 | `src-tauri/src/commands/settings.rs` | Extend `ConfigDto`/`FullConfigDto` + `get_config`/`save_full_config` to expose `providers` list and routing UI |
| P2 | `src-tauri/src/commands/settings.rs:182` | `rebuild_agent_if_model_changed` continues to work as display hint; no functional change needed |

---

## 7. Backward Compatibility

- Old `config.yaml` with only `[model]` block: `providers` defaults to `[]`, `routing` fields default to empty strings. `resolve_provider` falls straight through to `config.model.*` — **zero behavior change**.
- Config YAML round-trip test at `src/config/mod.rs:213` remains valid; add a parallel test for the new schema.
- The existing `config::set()` allowlist already silently drops unknown keys. New keys need explicit additions, or document CLI users must edit YAML directly.

---

## 8. Risks

1. **`call_llm` loads config on every turn** (`config::load()` at `agent.rs:179`). This is a disk read per message. Adding `resolve_provider` is free in terms of I/O since the load already happens, but a long-term perf improvement would be to pass `&Config` in rather than re-loading. Out of scope for MVP.

2. **`Agent.model` string drift.** The desktop `get_status` command reads `turn.agent.model` for display. Once routing takes precedence, this field may show the legacy model name even though a different model is actually used. Fix: either make `get_status` read from `resolve_provider` instead, or deprecate `agent.model` display.

3. **Gemini CLI image path** (`vision.rs:29`) constructs `format!("[image: {}] {}", image_path, question)` — this is not a standard CLI invocation format and its real behavior depends on the installed Gemini CLI version. The fix of using `config.vision.gemini_path` is trivial but the underlying CLI contract is undocumented.

4. **Provider-specific auth headers.** Some providers (Anthropic direct, Google Vertex) use non-Bearer auth (`x-api-key`, OAuth). The MVP design uses `Authorization: Bearer` universally, which is correct for OpenRouter, OpenAI, and most proxies. Anthropic direct and Vertex need a separate header-builder function if added later.

---

## 9. Decisions Needing a Verdict

**Decision A — Where does `modality` originate in the agent loop?**

Two options:
- Option 1 (simpler): `call_llm` always receives `"text"`; the API vision branch is invoked only from `execute_api` in the vision tool, which constructs its own `reqwest` call using `resolve_provider(config, "vision")` directly without going through `call_llm`. Pro: `call_llm` stays focused on text chat; no modality plumbing inside agent loop. Con: two separate HTTP client construction paths.
- Option 2 (unified): `call_llm` accepts `modality`, handles text and vision uniformly. Pro: one HTTP path. Con: `call_llm` must know how to build image-URL content parts, mixing concerns.

Recommendation: **Option 1** for MVP (cleaner separation), promote to Option 2 when streaming is added.

**Decision B — Default for `config.vision.provider` migration.**

Today's default is `"gemini-cli"`. If a user had already set `vision.provider = "gemini-cli"` explicitly in their YAML (from an old config write), the new code correctly keeps using Gemini CLI. If they set it to `"api"`, they need a valid `routing.vision` entry or they get a fallback error. This is the correct behavior but the error message must be actionable: name the missing config key explicitly.


## Brief: subgoals

Now I have a thorough picture of the codebase. Let me compile the brief:

---

# Subgoals for Goals — Design Brief

## 1. Current State

**File:** `src/core/goal_system.rs`

The hierarchy today is two levels deep:

```
Goal  {id, title, description, status, milestones: Vec<Milestone>, created_at, target_date}
  └─ Milestone  {id, title, description, completed: bool, tasks: Vec<String /*Task IDs*/> }
```

Tasks are stored separately in `TaskBoard.tasks: HashMap<String, Task>` keyed by UUID string. `Task` already has:
- `subtasks: Vec<Subtask>` (inline, not separate map entries — just `{id, title, completed: bool}`)
- `parent_id: Option<String>` — unused; nothing sets it

The intended Hermes-style chain `Goal → Milestone → Task → Subtask` is four levels, but in practice tasks and milestones are disconnected: milestones hold task ID strings but `TaskBoard` has no method that resolves them. `GoalDeliberator` is a pure in-memory struct; its `handle_goal_deliberate()` in `agent.rs:364` creates a local-scoped deliberator, collects stub strings, and discards everything when the function returns — no state survives.

**Persistence:** `TaskBoard` is `#[derive(Default)]` with no `save()`/`load()` methods. All goals and tasks vanish on restart. The SQLite pattern that works is in `src/memory/store.rs` (rusqlite 0.32, bundled feature).

**CLI surface:** No `Goals` sub-command exists. Goal-related operations are only reachable through the text-based `[TOOL:goal_deliberate:{...}]` protocol during a TUI/chat session. `default_tools()` in `agent.rs` registers `goal_deliberate` and `task` but nothing for CRUD on goals or subgoals.

---

## 2. The Missing Layer: Subgoals

**The gap:** Hermes-agent decomposes goals into *subgoals* (thematic sub-objectives, each ownable and independently trackable) before tasks. The current model skips this: `Milestone` is the closest analogue, but it has no status enum, no progress rollup from its tasks, and no recursive nesting. `Task.parent_id` is unused and could be repurposed.

**Design decision required (see §6 below):** flat-with-`parent_id` vs. explicit `Subgoal` struct nested in `Goal`.

---

## 3. Proposed Data Model (MVP)

### Option A — Flat `parent_id` on a unified `GoalNode`

Replace the `Goal + Milestone` duality with a single self-referential table. Clean for SQLite, ugly in Rust because you cannot borrow a recursive `Vec<GoalNode>` easily.

### Option B — Explicit `Subgoal` struct (recommended for MVP)

Add `Subgoal` between `Goal` and `Task`, keeping the existing `Task`/`Subtask` untouched:

```rust
// In src/core/goal_system.rs

pub struct Subgoal {
    pub id: String,                          // UUID
    pub goal_id: String,                     // FK → Goal.id
    pub title: String,
    pub description: String,
    pub status: GoalStatus,                  // reuse existing enum
    pub priority: Priority,                  // reuse existing enum
    pub task_ids: Vec<String>,               // references TaskBoard tasks
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

`Goal` gains: `pub subgoals: Vec<Subgoal>` (replacing `milestones`) plus a `progress()` method that rolls up from subgoals → tasks.

`TaskBoard` gains: `subgoals: HashMap<String, Subgoal>` for O(1) lookup.

Drop `Milestone` entirely (it is never rendered anywhere except `goal_progress()` and `format_board()`'s milestone count, and has no callers outside the file).

---

## 4. Persistence Design

### Storage format: JSON file (MVP), upgrade path to SQLite

**JSON at `~/.openassistant/goals.json`** is the minimal-viable choice:
- Zero new dependencies (`serde_json` is already in scope via `serde`/`reqwest`).
- `Goal`, `Subgoal`, `Task`, `Subtask` are all `Serialize + Deserialize` already.
- Atomic write: serialize to string, `std::fs::write(tmp) + rename`.
- Same pattern as Hermes's early session JSON before its migration to a session DB.

```rust
// New file: src/core/goal_store.rs

pub struct GoalStore {
    path: PathBuf,          // ~/.openassistant/goals.json
    pub board: TaskBoard,   // owns the in-memory state
}

impl GoalStore {
    pub fn open(data_dir: &str) -> Result<Self> { /* load or default */ }
    pub fn save(&self) -> Result<()> { /* atomic write */ }
    // convenience delegates: create_goal, create_subgoal, create_task, ...
}
```

`TaskBoard` needs `#[derive(Serialize, Deserialize)]` added (currently missing).

**SQLite later:** the schema would be three tables (`goals`, `subgoals`, `tasks`) with FK constraints, mirroring `src/memory/store.rs`'s pattern exactly. `rusqlite 0.32` is already a dep; no new crate needed. This is the P1 follow-up, not MVP.

---

## 5. Goal Deliberator + `goal_deliberate` Tool Integration

**Current problem:** `handle_goal_deliberate` in `agent.rs:364` instantiates a fresh `GoalDeliberator` each call, uses it for formatting only, then drops it. No goal is actually created in `TaskBoard`.

**MVP change:** the `goal_deliberate` handler should:
1. Call `GoalStore::open(&config.general.data_dir)` (synchronously; `rusqlite` / JSON I/O is sync).
2. Create a `Goal` with `status: Draft`, then create one `Subgoal` per deliberation role output (once real LLM calls are wired; for now at least persist the goal shell).
3. Call `store.save()`.
4. Return the goal ID in the output so the user can reference it.

**GoalDeliberator** stays in-memory (deliberation is ephemeral; its output populates the persisted subgoals). No structural change to `GoalDeliberator` needed for MVP.

---

## 6. Progress Rollup

```rust
impl Goal {
    pub fn progress(&self, board: &TaskBoard) -> GoalProgress {
        let subgoal_stats: Vec<_> = self.subgoals.iter().map(|sg| {
            let tasks: Vec<&Task> = sg.task_ids.iter()
                .filter_map(|id| board.tasks.get(id))
                .collect();
            let done = tasks.iter().filter(|t| t.status == TaskStatus::Completed).count();
            (done, tasks.len())
        }).collect();
        GoalProgress {
            subgoals_total: subgoal_stats.len(),
            subgoals_done: self.subgoals.iter().filter(|sg| sg.status == GoalStatus::Completed).count(),
            tasks_done: subgoal_stats.iter().map(|(d,_)| d).sum(),
            tasks_total: subgoal_stats.iter().map(|(_,t)| t).sum(),
        }
    }
}
```

---

## 7. CLI/Tool Surface (MVP)

### New CLI command: `Goals`

Add to `src/main.rs` `Commands` enum:

```rust
Goals {
    #[arg(long)] action: Option<String>,  // create|list|subgoal|task|complete|show
    #[arg(long)] goal: Option<String>,    // goal ID or title
    #[arg(long)] subgoal: Option<String>, // subgoal ID
    #[arg(long)] title: Option<String>,
    #[arg(long)] description: Option<String>,
}
```

Dispatch in `main()` — reads `GoalStore`, calls CRUD methods, saves, prints. Four sub-actions cover MVP: `create`, `list`, `subgoal` (add subgoal to a goal), `show` (tree view with progress rollup).

### New agent tools (additions to `default_tools()` in `agent.rs`)

| Tool name | Args | Purpose |
|---|---|---|
| `goal_create` | `{title, description}` | Create goal, persist, return ID |
| `goal_subgoal` | `{goal_id, title, description}` | Add subgoal to existing goal |
| `goal_list` | `{}` | List goals with progress |
| `goal_task` | `{subgoal_id, title, description}` | Create task linked to subgoal |

Each handler loads `GoalStore`, mutates, saves. The existing `goal_deliberate` handler is updated to additionally call `GoalStore::create_goal` and persist.

**Three touch-points per new tool** (per codebase convention): `handle_tool_calls` match arm, `default_tools()` list, and if the tool needs `ToolRegistry::execute` wired — check `src/tools/mod.rs` first (currently has no goal entries, so only `agent.rs` needs updating for these CRUD tools).

---

## 8. Files to Touch

| File | Change |
|---|---|
| `src/core/goal_system.rs` | Add `Subgoal` struct; add `subgoals: HashMap<String,Subgoal>` to `TaskBoard`; add `#[derive(Serialize,Deserialize)]` to `TaskBoard`; drop `Milestone` or keep as deprecated alias; add progress rollup |
| `src/core/goal_store.rs` | **New file.** `GoalStore` with JSON load/save; delegates CRUD to `TaskBoard` |
| `src/core/agent.rs` | Update `handle_goal_deliberate` to persist; add 3-4 new tool handlers; update `default_tools()` |
| `src/main.rs` | Add `Goals` command arm; add `use open_assistant::core::goal_store::GoalStore` |
| `src/lib.rs` | Re-export `goal_store` module |

No new Cargo dependencies for JSON storage. SQLite path would reuse rusqlite 0.32 already present.

---

## 9. Non-Goals (MVP cut)

- Real LLM calls from `GoalDeliberator` roles (each role calling `call_llm`) — kept as placeholder; this is a separate P1 that requires threading the `Agent` reference into the deliberator or using a channel-based callback.
- Recursive subgoal nesting (subgoals of subgoals) — the `Subgoal.goal_id` FK is sufficient for one level; deeper trees need a proper adjacency table in SQLite.
- Desktop UI surface for goals — `src-tauri/` is out of scope per CAPABILITY HONESTY TABLE; goals stay CLI/TUI only.
- FTS on goals — add to SQLite migration, not JSON MVP.
- Goal sharing / multi-user — out of scope.

---

## 10. Key Design Decisions Needing a Verdict

**Decision 1 — Drop `Milestone` or keep it?**

`Milestone` (line 364-371 of `goal_system.rs`) has `completed: bool` and `tasks: Vec<String>` but no status enum, no rollup, and no callers outside the file. Replacing it with `Subgoal` is clean but is a breaking API change in a public library crate. The safe path: keep `Milestone` with `#[deprecated]` and add `subgoals` alongside it; callers (zero external users today) migrate at their own pace. The clean path: delete it and update `create_milestone`/`complete_milestone`/`goal_progress` to operate on `Subgoal`. Recommend the clean path since the crate has no published version or external dependents.

**Decision 2 — JSON file vs. SQLite from day one?**

JSON at `~/.openassistant/goals.json` avoids schema migration headaches and reuses zero new infrastructure. SQLite from day one gives FTS, FK integrity, and matches the existing `memory.db` pattern — but requires writing ~60 lines of schema + CRUD boilerplate (similar to `src/memory/store.rs`). The project already has rusqlite bundled; the marginal cost of SQLite is low. However, JSON is sufficient for the stated MVP and is reversible: a `GoalStore::migrate_to_sqlite()` helper can import the JSON file later. **Recommended: JSON for MVP, SQLite in the next PR.**


## Brief: discord

I now have all the information needed to write a comprehensive brief. Here it is:

---

# Discord Bot Integration — Implementation Brief

## 1. Crate Decision: serenity vs poise vs twilight

### Options Evaluated

**serenity 0.12.5** (released 2025-12-20, ISC)
- Monolithic "batteries-included" framework: HTTP client, gateway WebSocket, cache, event handler system all in one crate.
- Default feature set (`default_no_backend`) includes `client + gateway + cache + http + model + builder + framework + standard_framework + utils`. TLS backend is separate (`rustls_backend` or `native_tls_backend`).
- Minimal feature set for read-messages-and-reply: `["client", "gateway", "model", "http", "builder", "rustls_backend"]` — omitting `standard_framework`, `cache`, `framework`, `utils` saves compile time and removes the slash-command machinery we don't need.
- Uses tokio internally; integrates cleanly with the existing `tokio = { version = "1", features = ["full"] }` dep.
- reqwest is an **optional** dep inside serenity (behind `http` feature), which serenity vendors internally — it will coexist with the project's own `reqwest 0.12` because both are optional deps pulled at the crate boundary.

**poise 0.6.2** (released 2026-04-13, MIT, built on serenity)
- Adds slash-command dispatch, prefix commands, and argument parsing on top of serenity. This project needs exactly none of that: it routes all messages to `Agent::process()`. poise is unnecessary complexity.

**twilight 0.17.1** (released 2025-12-13, ISC)
- A lower-level, "lego brick" ecosystem: `twilight-gateway`, `twilight-http`, `twilight-cache-inmemory` are separate crates, assembled by the caller. Requires ~5 crates, more boilerplate. Rust 1.89 minimum — currently above stable (project uses edition 2021 which implies 1.56+, but the actual MSRV in Cargo.toml is unconstrained). twilight does not add value here; it's suited for high-throughput bots, not a personal assistant.

**Verdict: serenity 0.12.5 with minimal features.** It is the standard choice for personal-assistant bots, has direct tokio integration, ships its own reqwest copy as an optional dep, and the event handler pattern maps naturally onto the "receive a message → call Agent::process() → send reply" loop.

---

## 2. Cargo.toml Addition

```toml
serenity = { version = "0.12", default-features = false, features = [
    "client",
    "gateway",
    "model",
    "http",
    "builder",
    "rustls_backend",
] }
```

Rationale for each feature:
- `client` — `Client` builder and event dispatch machinery
- `gateway` — WebSocket connection to Discord gateway (depends on `flate2`, already a common dep)
- `model` — `Message`, `UserId`, `ChannelId` types
- `http` — REST calls needed for `msg.channel_id.say()` (reply)
- `builder` — HTTP request structs, required by `http`
- `rustls_backend` — pure-Rust TLS via rustls; avoids native-tls system library requirement on Windows/Linux/Mac

`cache` is **intentionally excluded**. The bot only needs to read incoming messages and reply; it never queries "who is online" or resolves guild members by ID. Excluding cache eliminates `dashmap`, `parking_lot`, `rustc-hash` compile overhead and removes the memory model for a cache we never populate correctly in DM-only mode.

`standard_framework` and `framework` are excluded — the project handles routing itself via `Agent::process()`.

---

## 3. Privileged Gateway Intents

Discord requires opt-in for two privileged intents:

| Intent | Required for | Where to enable |
|---|---|---|
| `GUILD_MESSAGES` | Reading messages in servers | Not privileged — default |
| `MESSAGE_CONTENT` | Reading the text of messages | **Privileged** — must be enabled in Discord Developer Portal → Bot → Privileged Gateway Intents |
| `DIRECT_MESSAGES` | Receiving DMs | `GatewayIntents::DIRECT_MESSAGES` |

**`MESSAGE_CONTENT` is mandatory.** Without it, `msg.content` is empty for all messages except those mentioning the bot directly or sent in DMs. Enable it in the developer portal before testing.

In code:
```rust
use serenity::model::gateway::GatewayIntents;

let intents = GatewayIntents::GUILD_MESSAGES
    | GatewayIntents::DIRECT_MESSAGES
    | GatewayIntents::MESSAGE_CONTENT;
```

---

## 4. Exact Wiring Design

### 4.1 `discord::start()` signature change

Current stub at `src/gateway/discord.rs:5`:
```rust
pub async fn start(_token: &str) -> Result<()>
```
New signature:
```rust
pub async fn start(token: &str, config: crate::config::GatewayConfig, model_config: crate::config::ModelConfig) -> Result<()>
```
Pass `GatewayConfig` (for `discord_allowed_users`, `dm_policy`) and `ModelConfig` (for `api_key`, `api_base`, `model`) separately — avoids holding the full `Config` across `await` points (it does not implement `Send` if it contains `Arc`s, though currently it's a plain struct so cloning is fine).

### 4.2 Per-user Session State

`Agent::process()` takes `ctx: &mut FullContext` and `session: &mut Session`. Both must survive across Discord message events.

**Design: `Arc<Mutex<HashMap<UserId, SessionState>>>`** where:
```rust
struct SessionState {
    session: Session,
    ctx: FullContext,
}
```
- Key: Discord `UserId` (u64 newtype). Per-user rather than per-channel so DMs and server messages from the same user share one session (consistent memory). If per-channel isolation is preferred later, key on `(UserId, ChannelId)`.
- Stored in the serenity event handler struct as a shared-state field, passed as `Data` via serenity's `ClientBuilder::event_handler()`.

This is the same pattern used by `webchat.rs` (`Arc<Mutex<Vec<ChatMessage>>>`), so it is idiomatic within this codebase.

### 4.3 Event Handler Structure

```rust
// src/gateway/discord.rs

use std::collections::HashMap;
use std::sync::Arc;
use serenity::async_trait;
use serenity::client::{Client, Context, EventHandler};
use serenity::model::channel::Message;
use serenity::model::gateway::{GatewayIntents, Ready};
use tokio::sync::Mutex;

struct SessionState {
    session: crate::core::session::Session,
    ctx: crate::core::persona::FullContext,
}

struct Handler {
    agent: crate::core::agent::Agent,
    sessions: Arc<Mutex<HashMap<u64, SessionState>>>,
    allowed_users: Vec<String>,  // from GatewayConfig::discord_allowed_users
    dm_policy: String,           // from GatewayConfig::dm_policy
}
```

`message()` event dispatch:
```rust
#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, sctx: Context, msg: Message) {
        // 1. Ignore bot messages (prevent loops)
        if msg.author.bot { return; }
        // 2. Allowlist check (delegates to existing is_allowed())
        if !is_allowed(&msg.author.id.to_string(), &self.allowed_users) { return; }
        // 3. DM-policy gate (if dm_policy == "pairing", require user to be in allow list explicitly)
        // 4. Lock sessions map, get-or-insert SessionState for this user
        // 5. Call self.agent.process(&msg.content, &mut state.ctx, &mut state.session).await
        // 6. Send reply via msg.channel_id.say(&sctx.http, response).await
    }
}
```

### 4.4 Wiring in `gateway/mod.rs`

File: `src/gateway/mod.rs`, line 19 — remove the comment and change call site:
```rust
if !config.gateway.discord_token.is_empty() {
    info!("Discord token configured, starting Discord handler...");
    // Run discord on a spawned task so webchat can start concurrently
    let disc_config = config.gateway.clone();
    let model_config = config.model.clone();
    tokio::spawn(async move {
        if let Err(e) = discord::start(&disc_config.discord_token, disc_config, model_config).await {
            tracing::error!("Discord gateway error: {e}");
        }
    });
}
```
`webchat::start()` then proceeds on the main task. Both run concurrently.

### 4.5 Agent Construction in `discord::start()`

```rust
let agent = Agent::new(&model_config.model)
    .with_workspace(crate::config::data_dir_default())
    .with_tools_enabled(false);  // same as desktop: tools off unless user opts in
```
`tools_enabled: false` is the safe default for a bot serving potentially multiple users. The `GatewayConfig::dm_policy` field can gate this further.

---

## 5. Allowlist / DM Policy Wiring

`is_allowed()` at `src/gateway/discord.rs:12` already exists and is correct:
```rust
pub fn is_allowed(user_id: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() { return true; }
    allowed.iter().any(|id| id == user_id || id == "*")
}
```

`GatewayConfig::dm_policy` ("pairing" or "open") maps to:
- `"open"` — `is_allowed()` as-is (empty list means anyone can talk)
- `"pairing"` — treat empty `discord_allowed_users` as **deny-all** (override the `is_empty() → true` branch)

This is a one-line conditional at the top of the `message()` handler:
```rust
let effective_allow = &self.allowed_users;
let is_pairing = self.dm_policy == "pairing";
if is_pairing && effective_allow.is_empty() { return; }
if !is_allowed(&msg.author.id.to_string(), effective_allow) { return; }
```

`config::set()` at `src/config/mod.rs:183` already supports `"gateway.discord_token"`. Two keys need adding to the match: `"gateway.discord_allowed_users"` (comma-split) and `"gateway.dm_policy"`.

---

## 6. `FullContext` Construction

`Agent::process()` requires `&mut FullContext`. `FullContext` (from `src/core/persona.rs`) wraps `Persona + UserModel`. Each user gets their own `FullContext`:
```rust
use crate::core::persona::{FullContext, Persona, UserModel};
let ctx = FullContext {
    persona: Persona::load_or_default(&crate::config::data_dir_default()),
    user_model: UserModel::default(),  // grows over conversation via ctx.observe()
    // ... other fields per FullContext definition
};
```
Verify the exact `FullContext` struct fields (read `persona.rs` past line 139 for `FullContext`); the struct is defined there.

---

## 7. Files to Touch

| File | Change |
|---|---|
| `Cargo.toml` | Add `serenity = { version = "0.12", default-features = false, features = [...] }` |
| `src/gateway/discord.rs` | Full rewrite: add `Handler` struct, `EventHandler` impl, `start()` wiring, keep `is_allowed()` |
| `src/gateway/mod.rs` | Uncomment discord::start call (line 19), change signature, wrap in `tokio::spawn` |
| `src/config/mod.rs` | Add `"gateway.discord_allowed_users"` and `"gateway.dm_policy"` to `config::set()` match |

No changes needed to `src/core/agent.rs`, `src/core/session.rs`, or `src/core/persona.rs` — the existing API is sufficient.

---

## 8. MVP Cut vs Later

### MVP (P0)
- Cargo.toml: serenity dep with minimal features
- `discord::start(token, gateway_cfg, model_cfg)`: Handler + sessions HashMap + `message()` event
- Allowlist + dm_policy gate at top of `message()`
- `Agent::process()` call with per-user `SessionState`
- Reply via `channel_id.say()`
- `gateway/mod.rs`: uncomment + spawn

### P1 (after MVP works)
- Per-channel vs per-user session key decision (needs product input — see Decision 2 below)
- Typing indicator (`channel_id.broadcast_typing()`) while LLM is processing
- Message length splitting (Discord 2000-char limit; the agent can return multi-paragraph replies)
- Session persistence: serialize `Session` to SQLite using the existing `src/memory/store.rs` pattern so sessions survive bot restarts

### P2 / Non-Goals
- Slash commands — poise would help here, but this is not in the charter; text messages only
- Voice channels — excluded (`voice` serenity feature not added)
- Reaction handling
- Guild-side permission management
- Self-update via Discord command (already stubbed in agent tools but gated behind `tools_enabled`)

---

## 9. Risks

1. **`MESSAGE_CONTENT` privileged intent not enabled in the portal.** `msg.content` will be empty for all non-DM messages silently. Mitigation: log a warning at startup if the bot is in any guilds and `discord_allowed_users` is non-empty (indicates guild usage expected).

2. **serenity's internal reqwest vs the project's reqwest.** Both use `reqwest 0.12`. serenity vendors it as an optional dep. Because serenity uses `default-features = false`, serenity's reqwest instance is compiled separately with serenity's own feature set. No version conflict — cargo deduplicates if they resolve to the same semver. Low risk.

3. **`Mutex` contention on `sessions` HashMap.** A single `Arc<Mutex<HashMap>>` serializes all concurrent Discord messages. For a personal-assistant bot (low concurrency), this is fine. If extended to multi-user deployment, switch to `DashMap` (already an optional dep of serenity; could add separately) or per-user `Arc<Mutex<SessionState>>`.

4. **Session memory growth.** `Session::messages` is unbounded. `Agent::call_llm()` already trims to last 30 messages (line 168 of `agent.rs`), but the `Session` struct itself keeps all messages. For a long-lived bot, add a trim call after `agent.process()` or set a cap in `Session::add_message()`.

5. **`FullContext::observe()` writes are per-user but `Persona::load_or_default()` reads a single shared file.** Multiple users sharing one bot instance would get the same persona but divergent `UserModel`s. This is acceptable for the personal-assistant use case (single owner). Document in code.

---

## 10. Design Decisions Requiring a Verdict

**Decision 1: serenity feature set — include `cache` or not?**

Without `cache`, the bot cannot resolve `UserId → username` for logging without an extra REST call. With `cache`, serenity maintains an in-memory snapshot of all guild state (members, channels, roles) and `MESSAGE_CONTENT` works correctly. For DM-only use the cache adds memory with no benefit. For guild use, the cache is useful.

Recommended verdict: **exclude cache for MVP** (DM-first personal assistant); add it as a P1 feature if guild message support proves useful.

**Decision 2: Per-user or per-channel session keying?**

Keying by `UserId` means one continuous conversation thread per person across all channels/DMs — matches the Hermes/Honcho "user model grows over time" design intent. Keying by `(UserId, ChannelId)` gives isolated contexts per server channel, consistent with how most chat bots behave.

The existing `Session::new(channel, user_id)` takes both fields, so `Session` already supports either key strategy. The `HashMap` key is the only decision point.

Recommended verdict: **per-user (`UserId`) for MVP** to match the "persistent personal assistant" design. Allow `(UserId, ChannelId)` as a config option (`gateway.session_scope: "user" | "channel"`) in P1.

