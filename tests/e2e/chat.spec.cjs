const { test, expect } = require("@playwright/test");

// Inject a deterministic mock backend before the app's script runs. This
// mirrors the Rust command surface so we can exercise the real frontend logic
// (API-key gate, send flow, error rendering, clear) without Tauri.
async function installMock(page, { apiKeySet = false, failSend = false } = {}) {
  await page.addInitScript(
    ({ apiKeySet, failSend }) => {
      const state = {
        cfg: { model: "openrouter/owl-alpha", api_base: "https://openrouter.ai/api/v1", provider: "openrouter", api_key_set: apiKeySet, api_key_masked: apiKeySet ? "••••••••abcd" : "" },
        history: [],
        tools: false,
      };
      window.__MOCK_BACKEND__ = async (cmd, args) => {
        args = args || {};
        switch (cmd) {
          case "get_config": return { ...state.cfg };
          case "get_status": return { model: state.cfg.model, provider: "openrouter", mode: state.tools ? "tools" : "chat", workspace: "(mock)", message_count: state.history.length, tools_enabled: state.tools, api_key_set: state.cfg.api_key_set };
          case "get_history": return state.history.slice();
          case "clear_conversation": state.history = []; return null;
          case "set_tools_enabled": state.tools = !!args.enabled; return null;
          case "save_config":
            state.cfg.model = args.model; state.cfg.api_base = args.api_base;
            if (args.api_key) { state.cfg.api_key_set = true; state.cfg.api_key_masked = "••••••••" + String(args.api_key).slice(-4); }
            return null;
          case "send_message": {
            if (failSend) throw "LLM request failed: HTTP 401 — invalid key";
            const u = { id: "u" + state.history.length, role: "user", content: args.message, timestamp: "", metadata: null };
            const a = { id: "a" + state.history.length, role: "assistant", content: "Echo: " + args.message, timestamp: "", metadata: null };
            state.history.push(u, a);
            return a;
          }
          default: return null;
        }
      };
    },
    { apiKeySet, failSend }
  );
}

test("first run with no API key routes to Settings and shows the gate banner", async ({ page }) => {
  await installMock(page, { apiKeySet: false });
  await page.goto("/");
  await expect(page.getByTestId("view-settings")).toBeVisible();
  // Switch to chat: the banner prompts for a key and the input is disabled.
  await page.getByTestId("nav-chat").click();
  await expect(page.getByTestId("apikey-banner")).toBeVisible();
  await expect(page.getByTestId("chat-input")).toBeDisabled();
});

test("saving an API key clears the gate and enables chat", async ({ page }) => {
  await installMock(page, { apiKeySet: false });
  await page.goto("/");
  await page.getByTestId("settings-api-key").fill("sk-test-key-9999");
  await page.getByTestId("settings-save").click();
  await expect(page.getByTestId("save-status")).toHaveText(/Saved/);
  await page.getByTestId("nav-chat").click();
  await expect(page.getByTestId("apikey-banner")).toBeHidden();
  await expect(page.getByTestId("chat-input")).toBeEnabled();
});

test("sending a message renders user and assistant bubbles", async ({ page }) => {
  await installMock(page, { apiKeySet: true });
  await page.goto("/");
  await page.getByTestId("chat-input").fill("hello there");
  await page.getByTestId("chat-send").click();
  await expect(page.getByTestId("message-user")).toHaveText(/hello there/);
  await expect(page.getByTestId("message-assistant")).toHaveText(/Echo: hello there/);
});

test("a backend error is rendered as an error bubble, not a blank reply", async ({ page }) => {
  await installMock(page, { apiKeySet: true, failSend: true });
  await page.goto("/");
  await page.getByTestId("chat-input").fill("trigger failure");
  await page.getByTestId("chat-send").click();
  await expect(page.getByTestId("message-error")).toContainText(/HTTP 401/);
});
