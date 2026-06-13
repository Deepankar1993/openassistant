# Visible/Editable Memory Panel — "What I Know About You" — Design

Date: 2026-06-13
Status: Approved (next backlog item; user: "start on the next visible/editable memory panel, use agents")

## Context

The research lesson (Letta/MemGPT): memory only feels real and sells when users can
**see, edit, and forget** discrete items. Today "what the assistant knows about you" is
split across three disconnected stores (audit confirmed):

- **`UserModel`** (in `FullContext`) — auto-learned, **never persisted** (reset every
  process start), no per-item structure. Noisy; not the right surface to expose.
- **`MEMORY.md`** — freeform markdown, already editable in the desktop Memory view.
- **`MemoryStore`** (`src/memory/store.rs`) — a fully-built SQLite `entries` table
  (id, key, value, category, source, importance, timestamps) with FTS5 + triggers,
  **but `.store()` has zero callers** — completely dormant.

The smallest coherent way to deliver discrete, persisted, per-item-forgettable facts is
to **activate `MemoryStore`**: have the agent write facts to it, inject them into the
system prompt so they actually influence replies, and surface them in a panel with
edit/forget. `UserModel` stays session-scoped (out of scope here; noted as a follow-up).

## Architecture

### 1. `MemoryStore` additions (`src/memory/store.rs`)

The schema/triggers stay; add the missing pieces:
- A **sync** constructor `open_in(data_dir) -> Result<Self>` opening
  `<data_dir>/memory.db` (creates the dir with `std::fs`). The existing async
  `open`/`open_default` stay for current callers. (Rationale: the agent's sync
  `build_system_prompt` and the per-call command handlers want a cheap sync open,
  matching the conversation/watcher stores.)
- `list_all(limit) -> Vec<MemoryEntry>` — ordered `importance DESC, updated_at DESC`
  (drives both the panel and prompt injection).
- `update(id, value, importance) -> Result<usize>` — edits value + importance, bumps
  `updated_at`.
- `delete_by_id(id) -> Result<usize>` — precise one-click forget for the panel
  (existing `delete(key)` stays for the agent tool's key-based forget).
- **New FTS trigger** `entries_au AFTER UPDATE` (delete-then-insert into `entries_fts`)
  so edits don't leave the FTS index stale — there's currently only insert/delete
  triggers.

`open_default` is also pointed at config's data dir indirectly by switching desktop
callers to `open_in(&cfg.general.data_dir)` for consistency (today `open_default`
hardcodes `~/.openassistant`, which diverges if the user customized `data_dir`).

### 2. `remember` agent tool (`src/core/agent.rs`)

New tool so the dormant store gets populated and the panel has real content:
`{"action": "add|list|forget", "value": "...", "key": "<optional>", "category":
"fact|preference|...", "importance": 0.0-1.0}`.
- **add**: `value` required; `key` = provided or derived from the value (slug of the
  first few words, so it's human-readable and usable for `forget`); `category` default
  `"fact"`; `importance` clamped to [0,1], default 0.6; `source` `"agent"`. Returns a
  confirmation.
- **list**: `list_all(50)` formatted `key — value (importance)`.
- **forget**: by `key` → `delete(key)`; returns the count removed.
Opens `MemoryStore::open_in(&self.workspace_dir)`; best-effort (errors → readable text).
Wired into `execute_tool`, `default_tools()`, and the AcceptEdits/Auto permission
allowlists (low-risk local memory write, like `watch`).

### 3. Prompt injection (`src/core/agent.rs::build_system_prompt`)

Open `MemoryStore::open_in(&self.workspace_dir)`, take `list_all(20)`; if non-empty,
append a `# What I know about you` section (one `- value` line per fact, highest
importance first). Best-effort — a missing/locked DB never breaks prompt building.
Runs once per turn (before the tool loop), regardless of `tools_enabled` (manually-added
facts should still inform replies). A system-prompt nudge tells the model to call
`remember` when it learns a durable fact.

### 4. Desktop commands (`src-tauri/src/commands/memory.rs` + `lib.rs`)

All open `MemoryStore::open_in(&cfg.general.data_dir)`; return `Vec<MemoryEntry>`
(already `Serialize`; `id` + rfc3339 timestamps reach JS cleanly):
- `list_user_facts() -> Vec<MemoryEntry>` (`list_all(200)`)
- `add_user_fact(value, category, importance) -> ()` (source `"manual"`, key derived)
- `update_user_fact(id, value, importance) -> ()`
- `delete_user_fact(id) -> ()`
Registered in the single `generate_handler!`. `get_status` switched to `open_in` for
db-path consistency.

### 5. Desktop Memory view panel (`frontend/`)

Add a **"What I know about you"** section to the Memory view (alongside the existing
MEMORY.md/notes file browser — that stays). Each fact row: the value, a category chip,
an importance indicator, relative age; controls to **edit** (value + importance inline)
and **forget** (trash, `confirm()` → `delete_user_fact`). An **"+ Add fact"** input adds
a manual fact. Warm-editorial tokens, Lucide SVGs, no gradients/emoji chrome — matches
the rest. Extend the mock backend (`defaultMock`) with the four commands so Playwright +
plain-browser keep working; preserve all existing Memory-view testids.

WebChat is out of scope for the panel (chat-only surface), but the `remember` tool works
from every channel.

## Non-goals (deferred)

- Persisting `UserModel` / editing the structured profile (name, technical level…) — a
  separate concern; auto-learned data is noisy.
- Automatic post-turn fact *extraction* via an extra LLM pass — v1 relies on the agent
  choosing to call `remember` (deterministic, no added cost/latency).

## Error handling

Store open/read/write failures degrade gracefully: injection appends nothing, the tool
returns readable text, command handlers return an error string the UI shows. Never
breaks a turn.

## Testing (offline)

- `MemoryStore`: `open_in` round-trip; `list_all` importance ordering; `update` changes
  value/importance AND the row is still found by `search_fts` afterward (FTS trigger);
  `delete_by_id`; `store` returns id.
- Agent `remember` via `execute_tool` in a temp workspace: add → list shows it → forget
  removes it; non-add with missing value handled.
- Playwright: a new spec for the facts panel (add/list/edit/forget) against the mock;
  existing Memory-view tests stay green.
- `cargo test --workspace` green; clippy no new warnings.
