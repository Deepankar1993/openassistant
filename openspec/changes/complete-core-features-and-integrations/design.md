# Design Notes

Distilled from the research + three-persona panel (Industry Veteran, Senior Dev, Devil's Advocate) and lead-architect synthesis. Full briefs in `research-notes.md`.

## The linchpin: `call_llm_raw`

`Agent::call_llm` (`agent.rs:178`) is private, builds a fresh `reqwest::Client` per call, and reads `config.model.*` directly. Three features need a reusable LLM call: real workflow steps, real goal deliberation, and modality-routed text. So the first move is to extract a free function:

```rust
pub(crate) async fn call_llm_raw(
    client: &reqwest::Client,
    api_base: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
) -> anyhow::Result<String>
```

`Agent::call_llm` becomes a thin wrapper that resolves `(api_base, api_key, model)` via `resolve_provider(&config, "text")` and delegates. A single `reqwest::Client` is constructed once and shared (cloned `Arc` internally; `reqwest::Client` is already `Arc`-backed and cheap to clone). This is built in the workflow change and consumed by multi-model and subgoals — hence the hard ordering.

## Why dedicated databases (not `memory.db`)

`MemoryStore::open` enables neither `foreign_keys` nor WAL, and the FTS5 store is performance-sensitive. Checkpoint file snapshots are multi-MB UTF-8 blobs; workflow runs churn rows. Co-locating them would pressure the FTS page cache and risk cascade-delete misbehavior (FK enforcement is per-connection and off by default in SQLite). Decision: **`checkpoints.db`** and **`workflows.db`** are separate files under the data dir, each opened with `PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;` on every connection.

`CheckpointStore` and `WorkflowStore` stay **synchronous** (`Connection` is `!Send`): their call sites in `main.rs` are sequential and never `.await` while holding the connection. Workflow step results are written on the engine task in the join loop — **never** inside the `tokio::spawn`ed step closures, which must stay `Send`.

## Multi-model: opt-in routing, zero back-compat risk

`ModelConfig` has no struct-level `#[serde(default)]` (verified), so adding fields *to it* is unsafe. Instead, new **sibling** fields on `Config`:

```rust
#[serde(default)] pub providers: Vec<ProviderEntry>,
#[serde(default)] pub routing: RoutingConfig,
```

`ProviderEntry { name, api_base, api_key }`, `ModalityRoute { provider, model }`, `RoutingConfig { text, vision, image_gen, video }` — all `#[serde(default)]` + `Default`. `resolve_provider(config, modality)`:

1. Look up `routing.<modality>`. If `provider` is non-empty and matches a `providers[]` entry → return `(entry.api_base, entry.api_key, route.model)`.
2. Otherwise fall through to `(config.model.api_base, config.model.api_key, config.model.model)`.

**Empty routing reproduces today's behavior byte-for-byte**, including the desktop's `Agent::new(cfg.model.model)`. A regression test loads a legacy `[model]`-only YAML and asserts it round-trips and resolves to the legacy provider.

`config::set`'s allowlist is **not** widened — it is a deliberate guard against writing arbitrary `api_base` URLs from untrusted callers. Multi-provider setup is documented as a `config.yaml` edit.

## Checkpoint restore is a data-loss vector

The current `restore_checkpoint` does `std::fs::write` unconditionally — it will silently clobber unsaved edits made since the checkpoint. New behavior: before overwriting, hash the current on-disk file (SHA-256) and compare to the stored snapshot hash. If they differ, **skip with a warning** unless `force` is set. `list_checkpoints` returns metadata only; file bodies are loaded lazily via `load_checkpoint(id)`.

## Workflow honesty

The DAG executor is correct; only the step body is fake. Each step becomes one `call_llm_raw` call with a prompt assembled from the step `description` + the outputs of its `depends_on` steps. Partial failure is handled: a failing step records a `Failed` `StepResult` so dependents are detectably unsatisfiable and the engine emits "step X skipped: dependency Y failed" rather than the misleading "Workflow deadlock". The shipped `code-review` built-in is replaced with a tool-free `analyze → critique → summarize` chain — surfacing a tool-dependent workflow that can't run tools would just produce confident hallucinations.

## Subgoals: real output before persistence

Persisting a stub that emits placeholder text is building a database for fake data, so subgoals is sequenced **after** `handle_goal_deliberate` is rewired to call `call_llm_raw` per role. `Milestone` is dropped (all four callers are internal) in favor of a `Subgoal` layer; `TaskBoard` gains `Serialize`/`Deserialize` and a `subgoals` map. Persistence is `goals.json` written **atomically** via `tempfile::NamedTempFile::persist()` (plain `write`+rename is non-atomic and fails cross-volume on Windows). Single-writer is assumed and documented; SQLite is the next-PR upgrade.

## Discord: never hold a lock across `.await`

`serenity 0.12` with `default-features = false, features = ["client","gateway","model","http","builder","rustls_backend"]` — pin `rustls` to avoid shipping two TLS stacks; verify serenity's transitive `reqwest` dedups with ours and that its MSRV (~1.74) is satisfied. Per-user `Session` lives in `Arc<tokio::sync::Mutex<HashMap<UserId, SessionState>>>`. The critical pattern: lock → get-or-insert → **clone/take the state out → drop the guard** → `agent.process().await` → re-lock to write back. Holding the guard across the await would serialize all users (and won't compile with `std::sync::Mutex` in an async fn). Sessions are trimmed after each turn — unlike the one-shot CLI, the bot is long-running and `Session::messages` would grow unbounded. Discord is spawned with `tokio::spawn` and logs on error (it must not silently die behind the blocking `webchat::start()`).

## CLI-first; desktop stays honest

Implementation order: `update` → `checkpoint` → `call_llm_raw`+`workflow` → `multimodel` → `subgoals` → `discord`. No desktop affordance is added; the **CAPABILITY HONESTY TABLE** in `src-tauri/src/lib.rs` is updated to record which features are now real, so a later desktop change can surface them against a proven CLI path.

## Top risks (mitigations in the specs)

1. `run_cmd` swallowing a panicked build → `.await??`.
2. `update` workspace detection failing for `cargo install` binaries → `OPENASSISTANT_SRC` primary + clear error.
3. Tool-less `code-review` hallucinating → replace built-in.
4. `config.yaml` back-compat → `#[serde(default)]` everywhere + regression test.
5. Routing silently overriding `Agent::new(model)` → strictly opt-in.
6. Restore clobbering unsaved work → SHA-256 dirty check + `force`.
7. Discord lock-across-await → drop guard before `.await`.
8. JSON goals non-atomic write / concurrent race → `tempfile::persist()` + documented single-writer.
