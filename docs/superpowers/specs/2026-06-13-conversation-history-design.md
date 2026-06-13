# Conversation History — Design

Date: 2026-06-13
Status: Approved (autonomous continuation — next backlog item; user: "complete and use agents")

## Context

Both chat surfaces hold exactly one in-memory conversation that is lost on restart
(WebChat `Convo`, desktop `Turn`). Users expect a sidebar of past conversations they
can name, switch between, and delete — table stakes for a sellable chat product, and
the audit flagged "single conversation only" as a top gap on every surface.

Key enabler from the audit: `Session` (src/core/session.rs) already has a UUID `id`,
`created_at`/`updated_at`, and is fully `Serialize`/`Deserialize`. So a conversation's
id is simply its `session.id` — no parallel id scheme.

## Architecture

### 1. `ConversationStore` (new, `src/core/conversation_store.rs`)

SQLite, the `discord_store.rs` pattern (WAL, JSON blobs, opened per-operation — cheap
for a local single-user app, and sidesteps holding a `Connection` across `.await`).
DB at `<data_dir>/conversations.db`.

```sql
CREATE TABLE IF NOT EXISTS conversations (
    id           TEXT PRIMARY KEY,   -- == Session.id
    title        TEXT NOT NULL DEFAULT '',
    session_json TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
```

API (all `anyhow::Result`):
- `open_default(data_dir) -> Self` / `open(path) -> Self`
- `save(&Session, title: Option<&str>)` — `INSERT OR REPLACE`; when `title` is None and
  no title is stored yet, derive one from the first user message (`derive_title`,
  ≤ 48 chars, "New conversation" if empty). `updated_at = now`.
- `list_meta() -> Vec<ConversationMeta>` — `{id, title, updated_at, message_count}`,
  newest-updated first. `message_count` stored as a column too (cheap, avoids parsing
  every blob to list).
- `load(id) -> Option<Session>`
- `delete(id) -> bool`

`ConversationMeta` is `Serialize` (shared by Tauri commands + the WebChat JSON API).
`derive_title(&Session) -> String` is a pure, unit-tested helper.

Schema note: add `message_count INTEGER NOT NULL DEFAULT 0` to the table; `save`
writes `session.messages().len()`.

### 2. Persistence model (shared rule)

- A conversation is persisted lazily: **only after a turn produces messages.** Empty
  fresh sessions are never written, so the list has no blank entries.
- `FullContext` (persona + learned user-model) stays process-shared and is NOT
  per-conversation — the user model is about the user, not a chat, and it isn't
  persisted across restarts today either, so this is no regression. Only the `Session`
  (messages) is saved/restored. Switching swaps the active `Session`, keeping persona.

### 3. Desktop (`src-tauri`)

`Turn` is unchanged in shape (`agent`, `ctx`, `session`); the conversation id is
`turn.session.id`. The store is opened per command from `cfg.general.data_dir`.

After every turn (`send_message`, `send_message_stream`) → `store.save(&session, None)`.

New Tauri commands (registered in the single `generate_handler!`):
- `list_conversations() -> Vec<ConversationMeta>`
- `new_conversation() -> ()` — save current session if non-empty, then
  `turn.session = Session::new("desktop","local")`.
- `switch_conversation(id) -> Vec<Message>` — save current if non-empty; `store.load(id)`
  into `turn.session`; return its messages (frontend re-hydrates). Unknown id → error.
- `delete_conversation(id) -> ()` — `store.delete(id)`; if `id == turn.session.id`, start
  a fresh session.
- `clear_conversation()` (kept, same testid): redefined to "start a new conversation"
  — saves current then resets. Confirm copy updated.

`get_history`/`get_status`/`send_message*` unchanged except the added save.

### 4. WebChat (`src/gateway/webchat.rs`)

`Convo` gains nothing structural (it already wraps a `Session`); conversation id is
`convo.session.id`. Store opened per handler from `config.general.data_dir`. After each
turn (`send_message`, `chat_stream` on stream completion) → save.

New routes (the single-user `web` mutex still serializes everything):
- `GET /api/conversations` → `Vec<ConversationMeta>`
- `POST /api/conversations` → new: save current if non-empty, reset `web` convo, return `{}`
- `POST /api/conversations/select` body `{"id":"…"}` → load session into `web` convo,
  rebuild `convo.messages` (map `Message` → `ChatMessage`), return the `Vec<ChatMessage>`
- `DELETE /api/conversations/:id` → delete; if active, reset
- existing `/api/messages`, `/api/chat/stream` unchanged except added save.

On startup the `web` convo stays a fresh empty session (no auto-load), matching today.

### 5. Frontends

**WebChat (`src/gateway/webchat_page.html`)** — add a collapsible left sidebar:
"+ New chat" button + a list of conversations (title + relative time), active item
highlighted; click selects (fetches `/select`, re-renders), trash icon deletes
(confirm). Sidebar toggles on a hamburger in the header; off-canvas on mobile
(< 720px). Reuses the warm-editorial tokens already in the file. List refreshes after
each first-message-of-a-conversation (so a new chat appears once titled) and after
new/select/delete.

**Desktop (`frontend/`)** — add a conversation column inside the Chat view (between the
nav rail and the messages), same affordances. Extend the mock backend
(`defaultMock`) with the four new commands so Playwright + plain-browser keep working;
keep all existing testids (`message-list`, `clear-conversation`) intact.

## Error handling

Store failures (locked/corrupt DB) are logged and degrade to in-memory-only behavior —
a failed save never breaks a turn; a failed list returns empty. Unknown-id switch/delete
return a clear error to the caller.

## Testing (offline)

- `ConversationStore`: save/load/list/delete round-trip + ordering by `updated_at` +
  `message_count` accuracy, in a temp dir. `derive_title` (first user msg, truncation,
  empty → "New conversation"). Title not overwritten once set.
- Desktop command-layer test (the existing `commands` test pattern) for
  new/switch/list/delete against a temp data dir.
- Playwright e2e updated/extended for the desktop sidebar; existing chat assertions
  stay green.
- `cargo test --workspace` green; clippy no new warnings.
