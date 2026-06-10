# Screenshot-Ready Chat Implementation Plan

> **For agentic workers:** Executed inline (superpowers:executing-plans) with the two
> frontend rewrites delegated to parallel subagents (disjoint file sets). The event
> contract below is the source of truth for all parties.

**Goal:** Token streaming end-to-end + markdown/code rendering + warm-editorial design
refresh + tool-call timeline across WebChat and the desktop app; retire the fake
simulated-chat web UI.

**Spec:** `docs/superpowers/specs/2026-06-10-screenshot-ready-chat-design.md`

## Event contract (frozen)

`AgentEvent` serializes as JSON with `type` tag, snake_case:

```json
{"type":"token","text":"chunk"}
{"type":"tool_start","name":"bash","summary":"cargo test"}
{"type":"tool_end","name":"bash","ok":true,"preview":"running 50 tests…"}
{"type":"done","text":"final full assistant text"}
{"type":"error","message":"…"}
```

Transport: WebChat — `POST /api/chat/stream` body `{"content":"…"}`, response is SSE
(`data: <json>` lines; clients parse via fetch + ReadableStream since EventSource is
GET-only). Desktop — invoke `send_message_stream` (`{message}`), events arrive as Tauri
window event `chat-event` (payload = the JSON object above); the invoke promise still
resolves with the final `Message` for mock/fallback compatibility.

## Tasks

### Task 1: Core events (src/core/agent.rs) — TDD
- `AgentEvent` enum (Serialize+Clone, tag="type").
- `parse_sse_line(&str) -> Option<SseLine>` (`SseLine::{Done, Json(Value)}`) — unit
  tested (data prefix, [DONE], keepalive/garbage lines).
- `call_llm_stream(client, base, key, model, messages, tx)` — `"stream": true`,
  bytes_stream + line buffering, emits `Token` per delta, returns accumulated text;
  falls back to whole-body parse if the provider ignored `stream`.
- `process()` refactored to `process_inner(…, events: Option<&UnboundedSender<AgentEvent>>)`;
  new `process_events(…, tx)` wrapper emits `Done`/`Error` at the end. Tool dispatch
  emits `ToolStart` (name + summary: command for bash/shell, path for file ops, else
  truncated args) and `ToolEnd` (ok=false only for permission denial / dispatch Err;
  preview = first 200 chars).
- Dep: add `tokio-stream = "0.1"`.

### Task 2: WebChat consolidation (src/gateway/webchat.rs, src/ui/web.rs, src/main.rs)
- `GET /` serves `include_str!("webchat_page.html")` (placeholder page first; real page
  lands in Task 4A).
- `/vendor/marked.min.js`, `/vendor/purify.min.js`, `/vendor/highlight.min.js`,
  `/vendor/hljs-github.min.css`, `/vendor/hljs-github-dark.min.css` served via
  `include_str!` from `frontend/vendor/` with correct content-types.
- `POST /api/chat/stream` → Sse handler: spawn task (lock web convo, push user msg,
  `process_events`, push assistant msg on Ok), map rx → SSE events.
- `web` CLI command repointed to the gateway webchat server honoring `--port`;
  `src/ui/web.rs` simulated handler + INDEX_HTML removed.

### Task 3: Desktop streaming command (src-tauri)
- `send_message_stream(window, state, message)` — forwards events to
  `window.emit("chat-event", ev)`, returns final Message. Registered in the single
  `generate_handler!` in lib.rs.

### Task 4 (parallel subagents): frontends
- **4A WebChat page** (`src/gateway/webchat_page.html`): single-file page implementing
  the design system + streaming client + markdown pipeline + tool timeline + dark mode
  + stop button + auto-scroll escape hatch. History hydrate from `GET /api/messages`.
- **4B Desktop frontend** (`frontend/`): same design system; `chat-event` listener with
  invoke fallback; markdown pipeline shared semantics; keep all 5 views + onboarding
  working; keep `window.__MOCK_BACKEND__` Playwright hook working.

Design system (both): warm paper `#faf8f4` light / warm charcoal `#1a1816` dark, ochre
accent `#b45309`, NO gradients, no emoji chrome (inline Lucide SVGs), system serif
display stack for persona/headers, 15px/1.65 body, bubble-less assistant turns,
`marked` → `DOMPurify` → innerHTML (rAF-throttled while streaming, hljs highlight on
fence close, copy button + "Copied ✓"), `<details>` tool rows (collapsed on success).

### Task 5: Verify + docs
- `cargo test --workspace` green; clippy no new warnings; Playwright e2e (tests/e2e)
  green or updated alongside the frontend changes; CLAUDE.md web/UI sections updated.
