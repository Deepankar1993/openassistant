# Complete Core Features and Integrations

## Why

A capability audit of the shipped CLI surfaced three commands that *appear* to work but are facades, plus three commonly-requested capabilities that are scaffolded stubs. Verified against source on 2026-06-03:

- **`update`** (`src/main.rs:118`) prints `"Use 'cargo update && cargo build --release'"` and exits. Yet a **working** `SelfUpdater` already exists in `src/core/self_update.rs` (`check_update`, `update_from_source` via `git pull --rebase` + `cargo build`) — it is simply never called. The command is a placeholder over functioning code.
- **`workflow <name>`** (`src/core/workflows.rs:145`) resolves its dependency DAG correctly but each step's body is `format!("Step '{}' completed: {}")` — it `tokio::spawn`s a closure that returns a string and **never calls the LLM**. Users get fabricated "completed" output.
- **`checkpoint`** (`src/core/checkpoint.rs`) stores snapshots in a `Vec<Checkpoint>` that is **discarded on process exit**, despite the module doc claiming "Uses SQLite for persistence." `restore` is unreachable from the CLI (only `list` is half-wired). Every restart silently destroys all checkpoints.
- **Discord** (`src/gateway/discord.rs`) is a no-op (`info!("Discord gateway would start here")`); `gateway/mod.rs:19` comments out the start call. There is no `serenity` dependency.
- **Subgoals** — `goal_system.rs` models `Goal→Milestone` and `Task→Subtask` but `TaskBoard` is never persisted, and `handle_goal_deliberate` (`agent.rs:364`) emits `"In a full implementation..."` placeholder text instead of calling the LLM.
- **Multi-model** — `ModelConfig` is a single flat `{provider, model, api_key, api_base}`; `call_llm` (`agent.rs:178`) hardcodes `self.model` against the one configured provider. There is no way to route, e.g., vision to one provider and text to another.

The danger this change guards against is the same one `add-desktop-app` named: shipping affordances that *look* done. The fix is to wire real behavior into the agent core — CLI first — and to keep the desktop **CAPABILITY HONESTY TABLE** (`src-tauri/src/lib.rs:5`) accurate as stubs become real (and to NOT surface anything in the desktop until its CLI path is proven).

This proposal is the product of a research + three-persona adversarial review (Industry Veteran / Senior Dev / Devil's Advocate) and a lead-architect synthesis; the priority ranking, the hard sequencing constraint, and the data-loss/back-compat mitigations below are theirs.

## What Changes

Scope is **priority-gated**. P0 + P1 ship in this change; P2 is specified and sequenced but may land as a fast-follow if compile/time budget is exceeded. The hard sequencing constraint drives everything: **`call_llm_raw` (extracted in the workflow work) is the shared linchpin** reused by multi-model routing and by real goal deliberation — it is built first.

1. **`update` — wire the real source-updater (P0).** Replace the placeholder arm with a real flow over the existing `SelfUpdater`. `Commands::Update` gains `{ check: bool, yes: bool }`. Flow: detect the source checkout (`OPENASSISTANT_SRC` env var → walk up from `current_exe()` to a `Cargo.toml` with `name = "open-assistant"` → error naming the env var) → print current version → `check_pending()` (git `fetch` + `log HEAD..origin/HEAD --oneline`) → "up to date" exit if empty → `--check` prints and exits 0 → else confirm (unless `--yes`) → **dirty-tree gate** (`git status --porcelain`) → `update_from_source()` → message that the new binary is in `target/release/` and the running process keeps the old version until restart (and `cargo install --path .` if installed that way). The crates.io `check_update()` path is **dead** (crate unpublished) and is neutered with an explanatory `bail!` so it stops being a silent lie. The fire-and-forget bug in `run_cmd` (`self_update.rs:167`, `.await.unwrap_or(Ok(default Output))` turns a panicked build into fake success) is fixed to `.await??`.

2. **`checkpoint` — real SQLite persistence (P0).** `CheckpointStore` is rewritten over `rusqlite::Connection` against a **dedicated `~/.openassistant/checkpoints.db`** (NOT `memory.db` — multi-MB file snapshots would pressure the FTS page cache, and `MemoryStore::open` never enables `foreign_keys`). Schema: a `checkpoints` table + a `checkpoint_files` table with `ON DELETE CASCADE`; `PRAGMA journal_mode=WAL` and `PRAGMA foreign_keys=ON` on every `open()`. `restore_checkpoint` gains a **SHA-256 dirty-check**: if a target file's current content differs from the snapshot hash, it is skipped with a warning unless `--force` (stops silent clobbering of unsaved work). The CLI gains real `create` and `restore` actions (previously unreachable). The two existing in-memory tests are rewritten against a temp DB path. `list` returns lightweight metadata; a new `load_checkpoint(id)` fetches file bodies on demand.

3. **`workflow` — real LLM step execution (P1).** Extract `pub(crate) async fn call_llm_raw(client, model_config, messages) -> Result<String>` from `Agent::call_llm` (`agent.rs:178`); `call_llm` becomes a thin wrapper and a **single shared `reqwest::Client`** replaces the per-call `Client::new()`. `WorkflowEngine` gains `config: Arc<Config>` + a `WorkflowStore` (rusqlite, dedicated `workflows.db`) and a `new_with_config` constructor; the existing `Default`/`new` is kept (stub path). The fake spawn closure is replaced with a `call_llm_raw` call per step, with prior-step outputs injected as context; `StepResult`s are persisted in the join loop (on the engine task — `Connection` is `!Send`). Failed-dependency handling is corrected (record a `Failed` result and emit "step X skipped: dependency Y failed" instead of the misleading "Workflow deadlock"). The tool-less `code-review` built-in (which would hallucinate without tool dispatch) is replaced by an honest tool-free `analyze → critique → summarize` workflow.

4. **Multi-model routing (P1).** Add `providers: Vec<ProviderEntry>` and `routing: RoutingConfig` to `Config`, each with struct- **and** field-level `#[serde(default)]` so legacy `[model]`-only configs load with **zero behavior change**. A `resolve_provider(config, modality) -> (api_base, api_key, model)` resolves the credentials/model for a modality (`"text"`, `"vision"`, `"image_gen"`, `"video"`), falling through to the existing `config.model.*` when the route is empty. `call_llm_raw` consumes the resolved values; callers pass `"text"`. Routing is **strictly opt-in**: an empty `routing.text` reproduces today's exact behavior (and preserves the desktop's `Agent::new(cfg.model.model)` semantics). `image_gen`/`video` are parsed and config-ready but **not dispatched** (clearly P2); vision-API dispatch is also P2 (the existing Gemini-CLI vision tool is unaffected). `config::set`'s allowlist is deliberately **not** broadened (security-by-design against URL injection); multi-provider config is set by editing `config.yaml`.

5. **Subgoals (P2, blocked on #3's `call_llm_raw`).** `handle_goal_deliberate` is rewired to call `call_llm_raw` per deliberator role so it produces real output worth persisting. A `Subgoal` layer replaces `Milestone` (all callers internal: `create_milestone`/`complete_milestone`/`goal_progress`/`format_board`); `TaskBoard` becomes `Serialize`/`Deserialize` and is persisted to `~/.openassistant/goals.json` via a new `goal_store.rs` using **atomic write** (`tempfile::NamedTempFile::persist()`). New `goal_create`/`goal_subgoal`/`goal_list`/`goal_task` agent tools and a `Goals` CLI command surface it. SQLite persistence is the documented next-PR upgrade.

6. **Discord gateway (P2).** Add `serenity 0.12` (default-features off, `rustls_backend`). `discord.rs` is rewritten: a `Handler { agent, sessions, allowed_users, dm_policy }` whose `EventHandler::message` ignores bots, gates on the allowlist + `dm_policy`, and calls `Agent::process` per user. Per-user `Session` state lives in an `Arc<tokio::sync::Mutex<HashMap<UserId, SessionState>>>`; the guard is **taken/cloned out and dropped before `.await`** (never held across `process()`), and sessions are trimmed to bound growth (long-running, unlike the CLI). `gateway/mod.rs` uncomments the start with a `tokio::spawn` that logs errors. `config::set` learns `gateway.discord_allowed_users` and `gateway.dm_policy`.

7. **Keep the desktop honest.** As each stub becomes real, the **CAPABILITY HONESTY TABLE** in `src-tauri/src/lib.rs` is updated to reflect reality, but **no new desktop UI affordance is added in this change** — self-update needs a Tauri updater endpoint (separate milestone), and workflow/goals/Discord desktop surfaces wait until their CLI paths are proven. CLI-first, per the project owner's directive.

## Impact

**Affected specs (new capabilities):** `self-update`, `checkpoints`, `workflow-execution`, `multi-model-routing`, `goals`, `discord-gateway`.

**Affected / new code:**
- `src/main.rs` — `Update`, `Checkpoint`, `Workflow`, new `Goals` command arms.
- `src/core/self_update.rs` — `check_pending`, `is_dirty`, neutered `check_update`, `run_cmd` fix.
- `src/core/checkpoint.rs` — rusqlite rewrite + hash-checked restore + rewritten tests.
- `src/core/agent.rs` — extract `call_llm_raw` (shared `reqwest::Client`); `resolve_provider` wiring with a `modality` param; real `handle_goal_deliberate`; new `goal_*` tool handlers + `default_tools()` entries; `self_update` tool string.
- `src/core/workflows.rs` — `WorkflowStore`, `new_with_config`, real execution, failed-dep handling, replaced built-in.
- `src/config/mod.rs` — `ProviderEntry`/`ModalityRoute`/`RoutingConfig`, `resolve_provider`, `set` keys for Discord; back-compat round-trip test.
- `src/core/goal_system.rs` — `Subgoal` replaces `Milestone`; `TaskBoard` (de)serialize; `Goal::progress`.
- `src/core/goal_store.rs` — NEW: atomic JSON store.
- `src/core/mod.rs` — `pub mod goal_store;`.
- `src/gateway/discord.rs`, `src/gateway/mod.rs` — real serenity handler + wiring.
- `Cargo.toml` — `serenity 0.12`; promote `tempfile` to a dependency.
- `src-tauri/src/lib.rs` — update the CAPABILITY HONESTY TABLE.

**Back-compat / data notes:**
- Existing `config.yaml` files load unchanged (every new field defaulted). A regression test loads a legacy `[model]`-only config.
- `checkpoint --action list --id <session>` semantics change (`--id` now means a checkpoint id; `--session` means the session). Documented as a break.
- `goals.json` is single-writer; concurrent CLI invocations last-writer-win (documented; SQLite removes the race later).

## Non-Goals

- **Desktop UI for any of these features** — CLI-first this cycle; desktop surfacing is a documented follow-up gated by the CAPABILITY HONESTY TABLE.
- **Tauri self-update** — needs a published update endpoint + `tauri-plugin-updater`; out of scope.
- **Vision-API / image-gen / video dispatch** — the routing config is parsed and ready, but only `text` dispatch is wired in P1; the rest are explicit P2 stubs. The Gemini-CLI vision tool is untouched.
- **Broadening `config::set`'s allowlist** for `routing.*`/`providers.*`/`model.api_base` — intentional security boundary; edit `config.yaml`.
- **Multi-step tool/observe loop in workflows** — each step is a single LLM call, not a full `Agent::process` (avoids the per-step memory-write concurrency race the panel unanimously flagged).
- **SQLite goal persistence** — JSON for MVP; SQLite is the next-PR upgrade with a migration path.
- **Sub-agent (`task`) real execution, plugin marketplace, self-update UI** — remain stubs; not in scope.
