// Playwright tests for the conversation-history sidebar in the Chat view.
//
// These tests inject a self-contained mock backend that implements the four
// conversation commands (list_conversations, new_conversation,
// switch_conversation, delete_conversation) plus the chat commands the sidebar
// depends on. The shared mock-helpers.cjs intentionally leaves the new commands
// unimplemented (returns null), which the frontend already tolerates; this spec
// exercises the full sidebar behaviour against a backing store.

"use strict";

const { test, expect } = require("@playwright/test");

// Install a mock backend with conversation support. Mirrors the frontend's
// own defaultMock semantics: conversations persist lazily after the first
// completed turn.
async function installConvMock(page) {
  await page.addInitScript(() => {
    window.__MOCK_CALLS__ = [];
    const state = {
      api_key_set: true,
      model: "openrouter/owl-alpha",
      history: [],
      conversations: [], // { id, title, updated_at, messages }
      seq: 0,
      activeConvId: null,
    };
    const title = (msgs) => {
      const u = msgs.find((m) => m.role === "user");
      return (u && u.content) || "New conversation";
    };
    const persist = () => {
      if (!state.history.length) return null;
      let c = state.conversations.find((x) => x.id === state.activeConvId);
      if (!c) {
        c = { id: "conv-" + ++state.seq, title: "", updated_at: "", messages: [] };
        state.conversations.push(c);
        state.activeConvId = c.id;
      }
      c.messages = state.history.slice();
      c.title = title(c.messages);
      c.updated_at = new Date().toISOString();
      return c.id;
    };

    window.__MOCK_BACKEND__ = async (cmd, args) => {
      args = args || {};
      window.__MOCK_CALLS__.push({ cmd, args: JSON.parse(JSON.stringify(args)) });
      switch (cmd) {
        case "get_app_state":
          return { initial_view: "chat", api_key_set: state.api_key_set, user_name: "friend", data_dir: "~/.openassistant" };
        case "get_status":
          return {
            model: state.model, provider: "openrouter", mode: "chat",
            workspace: "(mock)", data_dir: "~/.openassistant",
            message_count: state.history.length, tools_enabled: false,
            api_key_set: state.api_key_set, memory_db_entries: 0, memory_md_chars: 0,
          };
        case "get_persona":
          return { name: "openAssistant", emoji: "🦞", tone: "friendly", language: "English", personality: "", principles: [], boundaries: [], capabilities: [] };
        case "get_history":
          return state.history.slice();
        case "send_message": {
          const u = { id: "u" + state.history.length, role: "user", content: args.message, timestamp: "", metadata: null };
          const a = { id: "a" + state.history.length, role: "assistant", content: "Echo: " + args.message, timestamp: "", metadata: null };
          state.history.push(u, a);
          persist();
          return a;
        }
        case "clear_conversation":
          persist();
          state.history = [];
          state.activeConvId = null;
          return null;
        case "list_conversations":
          return state.conversations
            .slice()
            .sort((a, b) => (b.updated_at || "").localeCompare(a.updated_at || ""))
            .map((c) => ({ id: c.id, title: c.title || title(c.messages), updated_at: c.updated_at, message_count: c.messages.length }));
        case "new_conversation":
          persist();
          state.history = [];
          state.activeConvId = null;
          return null;
        case "switch_conversation": {
          persist();
          const c = state.conversations.find((x) => x.id === args.id);
          if (!c) return [];
          state.history = c.messages.slice();
          state.activeConvId = c.id;
          return c.messages.slice();
        }
        case "delete_conversation":
          state.conversations = state.conversations.filter((c) => c.id !== args.id);
          if (state.activeConvId === args.id) { state.activeConvId = null; state.history = []; }
          return null;
        default:
          return null;
      }
    };
  });
}

test.describe("Conversation history sidebar", () => {
  test("conversation column and New button are present in the Chat view", async ({ page }) => {
    await installConvMock(page);
    await page.goto("/");
    await expect(page.getByTestId("conversation-column")).toBeVisible();
    await expect(page.getByTestId("conversation-new")).toBeVisible();
    // Empty state initially
    await expect(page.getByTestId("conversation-list")).toContainText(/No conversations yet/i);
  });

  test("a completed turn adds the conversation to the sidebar", async ({ page }) => {
    await installConvMock(page);
    await page.goto("/");
    await page.getByTestId("chat-input").fill("first message");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("message-assistant")).toHaveText(/Echo: first message/);
    // The conversation now appears in the sidebar, highlighted as active.
    const item = page.getByTestId("conversation-item").first();
    await expect(item).toBeVisible({ timeout: 5000 });
    await expect(item).toContainText(/first message/);
    await expect(item).toHaveClass(/active/);
  });

  test("New chat clears the message list and starts a fresh chat", async ({ page }) => {
    await installConvMock(page);
    await page.goto("/");
    await page.getByTestId("chat-input").fill("hello one");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("message-assistant")).toHaveText(/Echo: hello one/);

    page.on("dialog", (d) => d.accept());
    await page.getByTestId("conversation-new").click();

    // Message list returns to the empty starter state.
    await expect(page.locator("#message-list .empty-state")).toBeVisible();
    // The first conversation is still in the sidebar (persisted), now not active.
    await expect(page.getByTestId("conversation-item")).toHaveCount(1);
    await expect(page.getByTestId("conversation-item").first()).not.toHaveClass(/active/);

    // new_conversation was called.
    const calls = await page.evaluate(() => window.__MOCK_CALLS__.filter((c) => c.cmd === "new_conversation"));
    expect(calls.length).toBeGreaterThanOrEqual(1);
  });

  test("clicking a stored conversation switches and renders its messages", async ({ page }) => {
    await installConvMock(page);
    await page.goto("/");
    // Conversation A
    await page.getByTestId("chat-input").fill("alpha topic");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("message-assistant")).toHaveText(/Echo: alpha topic/);
    // Start a new one (B)
    page.on("dialog", (d) => d.accept());
    await page.getByTestId("conversation-new").click();
    await expect(page.locator("#message-list .empty-state")).toBeVisible();
    await page.getByTestId("chat-input").fill("beta topic");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("message-assistant")).toHaveText(/Echo: beta topic/);

    // Two conversations exist; click the alpha one.
    await expect(page.getByTestId("conversation-item")).toHaveCount(2);
    await page.getByTestId("conversation-item").filter({ hasText: "alpha topic" }).click();

    // Its messages render and switch_conversation was called.
    await expect(page.getByTestId("message-list")).toContainText(/alpha topic/);
    await expect(page.getByTestId("message-list")).not.toContainText(/beta topic/);
    const calls = await page.evaluate(() => window.__MOCK_CALLS__.filter((c) => c.cmd === "switch_conversation"));
    expect(calls.length).toBeGreaterThanOrEqual(1);
  });

  test("deleting the active conversation clears the chat to empty state", async ({ page }) => {
    await installConvMock(page);
    await page.goto("/");
    await page.getByTestId("chat-input").fill("delete me");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("message-assistant")).toHaveText(/Echo: delete me/);

    page.on("dialog", (d) => d.accept());
    const item = page.getByTestId("conversation-item").first();
    await item.hover();
    await item.getByTestId("conversation-delete").click();

    // Conversation removed and chat reset to empty state.
    await expect(page.getByTestId("conversation-item")).toHaveCount(0);
    await expect(page.locator("#message-list .empty-state")).toBeVisible();
    const calls = await page.evaluate(() => window.__MOCK_CALLS__.filter((c) => c.cmd === "delete_conversation"));
    expect(calls.length).toBeGreaterThanOrEqual(1);
  });
});
