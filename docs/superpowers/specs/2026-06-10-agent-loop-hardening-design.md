# Agent Loop Hardening — Design

Date: 2026-06-10
Status: Approved (autonomous goal session — user directed: research, decide, implement without intervention)

## Context

A codebase audit (three parallel exploration agents) found the agent core is real but
single-shot, and several adjacent systems are stubs. The five highest-value gaps,
chosen for coherence ("make the agent loop real and safe"), are:

1. **Known bug** — `PermissionRules::matches()` in `src/core/permissions.rs` strips the
   `Bash(` prefix but not the trailing `)`, so wildcard rules like `Bash(git *)` never
   match. Two pre-existing tests fail (`test_wildcard_matching`,
   `test_permission_rules_priority`).
2. **Single tool call per turn** — `Agent::process()` does one LLM call, executes at most
   one `[TOOL:...]`, appends the output to the reply, and stops. The model never sees
   tool output, so it cannot chain read → edit → verify.
3. **Permissions never enforced** — `PermissionMode`/`PermissionRules`/`PermissionManager`
   exist with correct logic but no call site in tool dispatch. All four gateway channels
   (Discord/Telegram/Slack/WebChat) now execute real tools, so remote users get ungated
   bash/file access whenever `tools_enabled` is on.
4. **`web_search` tool is fake** — the handler returns a Google URL string while a real
   multi-engine implementation (`src/core/web_search.rs`, DuckDuckGo + keyed engines)
   sits unused.
5. **`task` (sub-agent) tool is a stub** — returns "In a full implementation..." text,
   although everything needed to run an in-process child Agent already exists.

Deferred to a later batch (documented, not forgotten): Telegram/Slack session
persistence, hook fire points, standing-orders integration, MCP tool invocation,
streaming, cron.

## Approaches considered

**Tool loop:**
- (A) Keep the text-based `[TOOL:name:{json}]` protocol, add an iteration loop: feed
  each tool result back to the LLM until it replies without a tool call (max N rounds).
- (B) Switch to native OpenAI function calling.

Chosen: **A**. B is a protocol migration touching every prompt/parse path and behaves
inconsistently across OpenRouter-routed models; A is incremental, keeps all 20+ existing
handlers, and can be upgraded to B later behind the same `execute_tool` seam.

**Permission default:**
- (A) Enforce `Default` mode everywhere — breaks every existing CLI/TUI user (writes
  would be blocked headlessly).
- (B) Origin-aware: local front-ends keep today's behavior (`BypassPermissions`), remote
  gateway channels get a capped mode (`AcceptEdits` by default) — mirrors the existing
  claude-bridge origin hardening (commit 4e7c812). Deny rules from config apply at
  *every* mode, including bypass.

Chosen: **B**.

## Design

### 1. Wildcard fix (`src/core/permissions.rs`)

In `PermissionRules::matches`, strip the trailing `)` whenever the `Bash(` prefix was
stripped, for both `tool` and `rule`. Existing failing tests become the regression
tests; add one more for a non-`Bash(` wildcard (e.g. `mcp__*`).

### 2. Multi-step tool loop (`src/core/agent.rs`)

- Refactor `handle_tool_calls` into `execute_tool(&ToolCall, ...) -> Result<String>`
  that returns **only the tool output** (handlers stop concatenating `response + output`).
- `process()` becomes a loop (when `tools_enabled`):
  1. `call_llm(messages)`
  2. `parse_tool_call(response)` — none → this response is final, break.
  3. Permission check (see §3). Denied → the denial text becomes the tool result.
  4. `execute_tool(...)` → push the assistant text and a
     `[TOOL RESULT: name]\n<output>` user-role message onto the working message list
     *and* the session (so multi-turn context keeps the trajectory).
  5. Repeat, max `MAX_TOOL_ITERATIONS = 6`. On hitting the cap, append a note and return
     the last response.
- Tool outputs are truncated to 16 KiB before being fed back (guards the 30-message
  context window).
- `tools_enabled = false` path unchanged (single call, raw text).

### 3. Permission enforcement

- `Agent` gains `permissions: PermissionManager` (a `permission_mode` + config rules),
  default `BypassPermissions` to preserve current local behavior, with builder
  `with_permission_mode(PermissionMode)`.
- New `[permissions]` config section (serde-defaulted, additive — YAML round-trip safe):
  `gateway_mode` (string, default `"acceptEdits"`), `allow`/`ask`/`deny` (string lists).
- `PermissionManager::check` is called in the loop before `execute_tool`:
  - Rules are checked first at every mode — **deny beats bypass**.
  - `Ask` in a headless agent resolves to a refusal text ("requires interactive
    approval; not available on this channel") returned as the tool result so the model
    can adapt rather than the turn erroring.
  - For `bash`/`shell`, the check key is `Bash(<command>)` so command wildcards work;
    other tools check by name.
- Gateway channels (`discord`, `telegram`, `slack`, `webchat`) construct their Agent
  with `PermissionMode::from_str(&config.permissions.gateway_mode)`.

### 4. Real `web_search`

Replace the URL-formatting arm with a call into `crate::core::web_search` (same engine
selection logic the module already provides), formatting the top ~5 results as
`title — url\nsnippet`. Errors degrade to a readable message, never a turn failure.

### 5. Real `task` sub-agent

- `Agent` gains `depth: u8` (default 0). `handle_task_tool`:
  - depth ≥ 1 → refuse politely (no recursive fan-out in v1).
  - Build a child `Agent` (same model/workspace, `depth + 1`, same permission mode)
    whose `tools` are `default_tools()` filtered to the requested list
    (default: `read`, `glob`, `grep`).
  - Fresh `Session` + `FullContext`, system prompt prefixed with the sub-agent role
    description; run `process(prompt)` once — the child benefits from the §2 loop.
  - Return the child's final text labeled with the sub-agent type.

### Docs

Update CLAUDE.md: gateway section (all four channels are wired now), the failing-tests
note (fixed), the "one tool call per turn" description, and the stub list (task is real).

## Error handling

- Tool execution errors become tool-result text (the model sees and can recover),
  except transport/config errors from `call_llm`, which still propagate as `Err`.
- Sub-agent failure returns an error string as the tool result, never poisons the
  parent turn.

## Testing

- Permissions: the two failing tests now pass; add deny-beats-bypass, ask-resolves-to-
  refusal (headless), and `Bash(<command>)` key-building tests.
- Loop: unit-test `parse_tool_call` multi-candidate behavior and the iteration-cap and
  truncation helpers (LLM call itself is not unit-tested — no HTTP mocking layer exists).
- Sub-agent: depth-guard refusal test; tool-filter test.
- Config: `[permissions]` defaults + YAML round-trip (extends existing config tests).
- Full `cargo test --workspace` must pass (35 existing + new, 0 failures).
