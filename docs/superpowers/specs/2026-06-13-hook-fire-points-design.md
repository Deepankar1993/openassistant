# Hook Fire Points — Design (cluster batch 1 of 4)

Date: 2026-06-13
Status: Approved (user: "start with the cluster of half-wired internals … use agents")

## Context

`HookEngine` (`src/core/hooks.rs`) is complete — it loads `<data_dir>/.claude/hooks/
hooks.json`, fires shell commands per lifecycle event, passes a JSON `HookContext` on
stdin, honors timeouts, and parses a `{block, modified_input}` response so a
`PreToolUse` hook can block or rewrite a tool call. It just has **zero fire points** —
nothing in the agent loop ever calls `fire()`. This batch adds the fire points.

This is the first of four batches for the "half-wired internals" cluster (hooks →
standing orders → cron → MCP), each shipped and reviewed independently.

## Security model (the crux)

Hooks run **arbitrary shell commands**. They must fire only for the trusted local
operator (CLI/TUI/desktop), **never** for a turn driven by a remote channel — otherwise
a Discord/Telegram/Slack user could trigger the host's shell hooks. Mirrors the
claude-bridge `BridgeOrigin::Operator` pattern.

- Add `Agent.operator: bool` (default **false** = untrusted) + a `.operator()` builder.
- Local front-ends opt in: `ui/chat.rs` (TUI) and the desktop `build_core` call
  `.operator()`. Gateways (discord/telegram/webchat/slack) do **not** — so their agents
  never fire hooks.
- Sub-agents (`handle_task_tool`) are constructed without `.operator()`, so they never
  fire hooks either (no recursion / noise).

## Fire points (`src/core/agent.rs::process_inner`)

Load the engine once at the top of the turn: `self.operator` → `Some(HookEngine::
load_from_workspace(&self.workspace_dir))` (returns an empty engine if no file, so
firing is a cheap no-op in the common case), else `None`. A `None` engine fires nothing.

- **SessionStart** — once, when `session.messages()` is empty before the user message is
  added (first turn of a session).
- **UserPromptSubmit** — every turn, after the user message is added (`user_message` =
  the prompt).
- **PreToolUse** — in the tool loop, after `ToolStart` is emitted and before
  `check_permission`. `tool_name` + `tool_input` = the parsed call. Apply the response:
  - any result with `block: true` → skip execution; the tool result becomes
    `"⛔ Blocked by a PreToolUse hook"` (+ the hook's stderr if any), `ok = false`, so
    the model adapts. Permission check and `execute_tool` are not run.
  - else the last result's `modified_input` (if any) replaces `tool_call.arguments`
    before the permission check + execution (so rewrites are still gated).
- **PostToolUse** / **PostToolUseFailure** — after the tool output is known: fire
  `PostToolUse` when `ok`, else `PostToolUseFailure`; `tool_name` + `tool_output` =
  the (untruncated) result.
- **Stop** — once at end of turn, before returning; `assistant_message` = the final
  response.

Hook execution is best-effort and side-effecting: results are logged; only `PreToolUse`
`block`/`modified_input` change control flow. Hook failures/timeouts never abort the
turn (the engine already returns error `HookResult`s rather than panicking).

The block/modify decision is factored into a pure helper
`decide_pre_tool(results) -> PreToolDecision { blocked, reason, modified_input }` so it
is unit-testable without spawning bash.

## Non-goals

- No new hook events beyond what's wired here (SubagentStop/PreCompact/Notification/
  MessageDisplay stay defined-but-unfired — no natural call site yet).
- No hooks-management CLI/UI; `hooks.json` is the interface (Claude-Code parity).
- Streaming `process_events` shares `process_inner`, so hooks work there too — no
  separate streaming path.

## Testing

- `decide_pre_tool`: block wins over modify; modified_input applied when not blocked;
  empty results → no-op.
- `HookEngine::fire` end-to-end against a temp `hooks.json` (bash is available in CI):
  a `PreToolUse` hook emitting `{"block":true}` yields `block==true`; a plain hook runs
  and captures stdout/exit code.
- An `operator`-gating test: an `Agent` without `.operator()` loads no hooks (helper
  returns `None`); with `.operator()` it returns `Some`.
- `cargo test --workspace` green; clippy no new warnings.
