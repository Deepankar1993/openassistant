# Tasks

Priority tags: **[P0]** ships first, **[P1]** ships in this change, **[P2]** specified + sequenced (fast-follow if budget exceeded).

> **Shared-dependency note:** Task 10 (`call_llm_raw`) gates tasks 19 and 21. Build the P1 workflow extraction before multi-model wiring and real goal deliberation, even though multi-model is also P1.

## update (P0)
- [x] 1. [P0] Fix `run_cmd` in `self_update.rs`: propagate with `.await??` (remove the `ExitStatus::default()` fake-success fallback).
- [x] 2. [P0] Add `check_pending()` (`git fetch` + `git log HEAD..origin/HEAD --oneline`) and `is_dirty()` (`git status --porcelain`) to `SelfUpdater`; neuter `check_update()` with an explanatory `bail!`.
- [x] 3. [P0] Add workspace-dir detection (`OPENASSISTANT_SRC` env → `current_exe()` parent walk for `Cargo.toml` `name = "open-assistant"` → error naming the env var) in `main.rs`.
- [x] 4. [P0] Change `Commands::Update` to `{ check, yes }`; implement the flow incl. `--yes`/stdin prompt, dirty gate, and the post-build "restart / `cargo install --path .`" message.
- [x] 5. [P0] Update the `self_update` agent-tool response string in `agent.rs` to point at `openassistant update --check`.

## checkpoint (P0)
- [x] 6. [P0] Rewrite `CheckpointStore` over `rusqlite::Connection`; `open`/`open_default` for `checkpoints.db` with `journal_mode=WAL` + `foreign_keys=ON`.
- [x] 7. [P0] Implement `create_checkpoint` (transaction + prune via `DELETE … LIMIT` subquery), `restore_checkpoint` (SHA-256 dirty-check + `force` flag), `list_checkpoints` (metadata), `load_checkpoint`, `latest_checkpoint`, `format_checkpoints`.
- [x] 8. [P0] Rewrite the two existing checkpoint tests against a temp DB path.
- [x] 9. [P0] `main.rs`: `Checkpoint { action, id, session, workspace, files, description, force }`; wire `list`/`create`/`restore`; `--session` defaults to a workspace-path hash.

## workflow (P1)
- [x] 10. [P1] Extract `pub(crate) async fn call_llm_raw(client, model_config, model, messages)` in `agent.rs`; `Agent::call_llm` delegates; share one `reqwest::Client`.
- [x] 11. [P1] Add `WorkflowStore` (rusqlite, `workflows.db`, WAL + FK) with `open`/`upsert_run`/`upsert_step`/`list_runs`/`get_run`.
- [x] 12. [P1] Add `config: Arc<Config>` + `db` fields and `new_with_config` to `WorkflowEngine`; keep `Default`/`new`.
- [x] 13. [P1] Replace the fake spawn closure with `call_llm_raw` + prior-step output injection; persist `StepResult`s in the join loop (off the spawn).
- [x] 14. [P1] Fix failed-dependency handling (record `Failed`, "skipped: dep failed" instead of "deadlock"); fix `list_runs()`; add `get_run_status`.
- [x] 15. [P1] Replace the tool-less `code-review` built-in with an honest `analyze → critique → summarize` workflow.
- [x] 16. [P1] `main.rs`: load config, `Arc`, call `new_with_config`; print run status.

## multimodel (P1)
- [x] 17. [P1] Add `ProviderEntry`/`ModalityRoute`/`RoutingConfig` (struct + field `#[serde(default)]`, `Default`); extend `Config`.
- [x] 18. [P1] Add `resolve_provider(config, modality) -> (api_base, api_key, model)`.
- [x] 19. [P1] Wire `call_llm_raw` to use `resolve_provider`; opt-in (empty routing ⇒ current behavior); callers pass `"text"`.
- [x] 20. [P1] Add a backward-compat YAML round-trip test loading a legacy `[model]`-only config.

## subgoals (P2 — after task 10)
- [x] 21. [P2] Make `handle_goal_deliberate` call `call_llm_raw` per deliberator role (real output).
- [x] 22. [P2] Add `Subgoal`; replace `Milestone`; migrate `create_milestone`/`complete_milestone`/`goal_progress`/`format_board`; add `Goal::progress`.
- [x] 23. [P2] `#[derive(Serialize, Deserialize)]` on `TaskBoard`; add `subgoals` map.
- [x] 24. [P2] New `goal_store.rs` (atomic JSON via `tempfile::persist()`); `pub mod goal_store;` in `core/mod.rs`; promote `tempfile` to a dependency.
- [x] 25. [P2] Add `goal_create`/`goal_subgoal`/`goal_list`/`goal_task` tools (handler + `default_tools()` only); persist via `GoalStore`.
- [x] 26. [P2] `main.rs`: `Goals { action, goal, subgoal, title, description }` command.

## discord (P2)
- [x] 27. [P2] Add `serenity 0.12` (default-features off, rustls); verify reqwest/TLS dedup + MSRV.
- [x] 28. [P2] Rewrite `discord.rs`: `Handler`, `EventHandler::message` (bot-ignore, allowlist + dm_policy gate, tokio-Mutex session map with guard dropped across `process().await`, session trim), `start(token, gateway_cfg, model_cfg)`.
- [x] 29. [P2] `gateway/mod.rs`: uncomment + `tokio::spawn` with error logging.
- [x] 30. [P2] `config::set`: add `gateway.discord_allowed_users` and `gateway.dm_policy`.

## verification
- [x] 31. `cargo build --workspace` clean.
- [x] 32. `cargo test --workspace` (new + existing tests; note the two pre-existing `permissions.rs` failures are unrelated).
- [x] 33. Update the CAPABILITY HONESTY TABLE in `src-tauri/src/lib.rs`.


