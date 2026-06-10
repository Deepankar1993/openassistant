# Screenshot-Ready Chat — Design

Date: 2026-06-10
Status: Approved (autonomous goal session — user directed: research viral/sellable features + UI/UX, implement, use agents)

## Context

Three research agents (market/viral analysis, UI surface audit, UX best practices)
converged on the same conclusion: the highest-impact sellable improvement is making
the chat surfaces look and feel like a real product. The #1 credibility signal in
shared screenshots is a polished chat UI with streaming and highlighted code; the
current surfaces render plain text, wait-then-dump full responses, use purple
gradients + emoji chrome (documented "AI-slop tells"), and have no dark mode.

**This batch:** token streaming end-to-end, sanitized markdown + code blocks with
copy buttons, a distinctive warm-editorial design refresh with dark mode, and a
collapsible tool-call timeline (enabled by the multi-step tool loop shipped earlier
today).

**Deferred (recorded in memory):** proactive Daily Brief over Discord/Telegram (the
other top viral bet — backend batch), conversation-history sidebar, TUI markdown,
voice, RAG workspace, marketplace.

## Surfaces in scope

- **WebChat** — served by the gateway (`src/gateway/webchat.rs`) using the page in
  `src/ui/web.rs`. Gets: SSE streaming endpoint, new frontend.
- **Desktop (Tauri)** — `frontend/` + `src-tauri/src/commands/chat.rs`. Gets:
  chunk events via Tauri `emit`, new frontend.
- **TUI** — out of scope this batch.

## Architecture

### 1. Core: event-emitting agent turn (`src/core/agent.rs`)

New public enum:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Token { text: String },
    ToolStart { name: String, summary: String },
    ToolEnd { name: String, ok: bool, preview: String },
    Done { text: String },
    Error { message: String },
}
```

- `call_llm_stream(client, api_base, api_key, model, messages, tx)` — same POST as
  `call_llm_raw` plus `"stream": true`; parses SSE lines (`data: {...}`,
  `choices[0].delta.content`), forwards each token via
  `tokio::sync::mpsc::UnboundedSender<AgentEvent>`, returns the full accumulated
  text. `[DONE]` sentinel ends the stream. On providers that ignore `stream`,
  falls back gracefully (whole body parsed as one chunk → one Token event).
- `Agent::process_events(message, ctx, session, tx)` — same loop as `process()`
  (permission gate, execute_tool, trajectory recording, 6-round cap) but each LLM
  call streams tokens, and tool dispatch emits `ToolStart`/`ToolEnd`
  (`preview` = first 200 chars of output). Ends with `Done { text: final }`.
  `process()` remains unchanged for non-streaming callers (Discord/Telegram/Slack
  post whole messages anyway).
- Token events during a response that turns out to contain a `[TOOL:...]` call are
  fine: the frontend replaces the in-flight text with the parsed-out version when
  the tool step arrives (the raw `[TOOL:...]` markup is stripped client-side for
  display; v1 keeps server simple).

### 2. WebChat: consolidate to ONE real web UI (`src/gateway/webchat.rs`)

Audit finding: the `web` command's pretty UI (`src/ui/web.rs`) is a **fake** —
`handle_chat` returns hardcoded "This is a simulated response" text and never
calls the agent — while the real gateway webchat serves no HTML at all. Shipping
a simulated-chat surface is exactly the toy-signal this batch exists to kill.

- The gateway webchat becomes THE web UI: `GET /` serves the new page (a single
  HTML file at `src/gateway/webchat_page.html`, embedded via `include_str!`).
- New route `POST /api/chat/stream` → `axum::response::sse::Sse`. Handler spawns
  `process_events` with an unbounded channel; each `AgentEvent` is one SSE
  `data:` JSON line. Existing `/api/messages` JSON API stays (programmatic use).
- Vendored `marked.min.js`, `purify.min.js`, `highlight.min.js` + hljs theme css
  served at `/vendor/*` via `include_str!` from `frontend/vendor/` (single copy
  shared with the desktop app).
- The `web` CLI command is repointed to start the gateway webchat server (with
  its port flag); `src/ui/web.rs`'s simulated handler and embedded page are
  removed.

### 3. Desktop streaming (`src-tauri/src/commands/chat.rs` + `frontend/app.js`)

- New command `send_message_stream(message)` — spawns `process_events`; forwards
  each event as a Tauri window event `chat-event` (JSON payload). Returns the final
  text (so the invoke promise still resolves for compatibility/mocks).
- `frontend/app.js` listens via `window.__TAURI__.event.listen('chat-event', ...)`;
  falls back to non-streaming `send_message` when Tauri events are unavailable
  (plain-browser mock mode).

### 4. Frontend design system (both surfaces)

"Warm editorial workshop" direction from the UX research:

- **Type:** display serif via system stacks (no font binaries in repo — YAGNI):
  `Iowan Old Style, Palatino Linotype, Georgia, serif` for persona name/headers;
  system grotesque for UI/body (15px/1.65); `JetBrains Mono, Cascadia Code,
  Consolas, monospace` for code. Message column max ~72ch.
- **Color:** warm paper light theme (`#faf8f4`) / warm charcoal dark (`#1a1816`),
  single burnt-ochre accent (`#b45309` range), **no gradients**. Assistant turns
  bubble-less (flat on background, serif name label); user turns subtly tinted
  blocks. Dark/light via CSS custom properties + `prefers-color-scheme` + manual
  toggle persisted to localStorage.
- **Chrome:** Lucide inline SVGs replace all emoji chrome; one radius token; 8px
  grid; 24px+ between turns; 120–180ms ease-out motion only;
  `prefers-reduced-motion` respected.
- **Markdown pipeline:** `marked` → `DOMPurify.sanitize` → innerHTML. During
  streaming re-render the in-flight message at most every animation frame; code
  fences render as plain `<pre>` until closed, then `hljs.highlightElement` + a
  header bar (language label + copy button with "Copied ✓" feedback).
- **Tool timeline:** each ToolStart/ToolEnd renders a compact row in the assistant
  turn — spinner→check/cross SVG, tool name, summary — as a `<details>` collapsed
  on success, expanded on failure, full output in a code block.
- **Behaviors:** stop-generation button while streaming (client closes the SSE/
  ignores events; server task aborts on channel close), auto-scroll that stops
  following when the user scrolls up (+ "jump to bottom" pill), Enter/Shift+Enter,
  error rows with Retry, `aria-live="polite"` stream container.

## Error handling

- Stream transport failures emit `Error` then close; frontends render an inline
  error row with Retry.
- Channel-closed (client disconnected / stop pressed) aborts the agent task —
  `send` failures break the loop server-side.
- Vendored libs missing → graceful degradation to escaped-plaintext rendering.

## Testing

- Rust: unit tests for SSE chunk parsing (`parse_sse_chunk`) and AgentEvent JSON
  shape; existing 50 tests keep passing.
- Playwright e2e (existing harness in tests/e2e): chat renders markdown, copy
  button appears on code blocks, theme toggle persists (desktop frontend, mock
  backend).
- Manual: `cargo run -- gateway` + browser screenshot of streaming + tool timeline.
