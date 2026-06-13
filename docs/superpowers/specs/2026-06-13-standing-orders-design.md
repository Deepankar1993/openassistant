# Standing Orders Integration — Design (cluster batch 2 of 4)

Date: 2026-06-13
Status: Approved (cluster continuation; user: "continue")

## Context

`StandingOrdersEngine` (`src/core/standing_orders.rs`) parses/holds standing orders with
triggers (Keyword, EveryNMessages, SessionEnd, …) and actions (InjectContext, SaveNote,
RunCommand, Webhook, RunTool, RunSkill, SendMessage). It ships 3 hardcoded defaults
(auto-remember preferences, project-tracker, session-summary) but is **never constructed
or called** from the loop, has **no persistence**, and its `check_message` only collects
`InjectContext` — `SaveNote` is a debug-log no-op. This batch wires the safe, useful part
into the loop with persistence + a CLI.

## Scope decision (what's safe to auto-fire)

A standing order fires on the user's *message content*, so on a gateway channel a remote
user's text drives it. Therefore:

- **Auto-fire on every origin** (safe, local-only side effects): `InjectContext` (adds a
  line to the system prompt for this turn) and `SaveNote` (appends a rendered note to the
  daily memory file — the agent already logs every turn to daily notes, so this is no new
  exposure). These are exactly what the 3 defaults use.
- **Operator-gated** (arbitrary code/network, like hooks): `RunCommand` (shell) and
  `Webhook` (HTTP POST) execute only when `Agent.operator` (CLI/TUI/desktop). On a remote
  turn they are skipped with a `warn!`.
- **Deferred / logged no-op** in v1: `RunTool`, `RunSkill`, `SendMessage` — they need
  re-entrancy into the tool loop / a channel handle the engine doesn't have. Logged as
  "not auto-executed; use a hook or the tool directly." Honest, no dead-but-dangerous path.

## Architecture

### `standing_orders.rs`
- `StandingOrdersEngine::load(data_dir) -> Self` — read `<data_dir>/standing_orders.json`;
  if absent, seed `new()` defaults and `save`. `save(data_dir) -> Result<()>` (atomic via
  tempfile, the goal-store pattern). `add_order/remove/list` already exist.
- `matched(&self, message, message_count) -> Vec<StandingOrder>` — orders whose Keyword or
  EveryNMessages trigger fires for this message (clones; the agent executes the actions).
- `session_end_orders()` already exists (rename-free; returns SessionEnd orders).
- `render_template(template, message, message_count) -> String` — pure helper replacing
  `{{message}}` and `{{message_count}}`. Unit-tested.

### Agent (`src/core/agent.rs::process_inner`)
- Load the engine once per turn off-thread (`spawn_blocking`, like hooks/facts).
- After the user message is added, compute `matched(...)` and apply each action:
  - `InjectContext` → push to a `standing_context: Vec<String>`.
  - `SaveNote` → `mem.append_daily(render_template(...))`.
  - `RunCommand`/`Webhook` → execute only if `self.operator`, else `warn!`+skip.
  - others → `debug!` skip.
- `build_system_prompt` gains `standing_context: &[String]` → a `# Standing context`
  section (rendered before the tool list).
- At the Stop point (end of turn), apply `session_end_orders()` (SaveNote → daily note,
  with `{{message_count}}` = session length).

### CLI (`src/main.rs`)
- `openassistant standing-orders` (alias `orders`): `--action list|add|remove`,
  `--text "when i mention X, then Y"` (uses `parse_from_text`), `--id <id>`. Loads,
  mutates, saves the JSON. Listing shows id/name/enabled/trigger summary.

## Error handling

A missing/corrupt `standing_orders.json` falls back to defaults (logged), never breaks a
turn. Note writes and command/webhook executions are best-effort (logged on failure).

## Testing

- `render_template`: both placeholders, missing placeholders, repeated.
- `matched`: keyword case-insensitive hit/miss; EveryNMessages modulo; disabled orders
  skipped; SessionEnd not returned by `matched`.
- `load`/`save` round-trip in a temp dir (seeds defaults when absent; reloads custom).
- An agent test: a temp workspace with a custom InjectContext keyword order →
  `matched` returns it; a SaveNote order writes a daily note (assert the file content).
- `cargo test --workspace` green; clippy no new warnings.
