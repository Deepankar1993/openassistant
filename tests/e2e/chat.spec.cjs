const { test, expect } = require("@playwright/test");
const { installMock } = require("./mock-helpers.cjs");

// Chat view tests.  The shared installMock helper handles get_app_state so that
// the new wizard-based boot flow is correctly bridged:
//   apiKeySet: true  → initial_view "chat"  → no wizard, Chat view shown
//   apiKeySet: false → initial_view "onboarding" → wizard opens
//
// The first test intentionally covers that "no API key" now routes to the
// onboarding wizard (behaviour changed from old Settings-redirect path).
// Tests 2-4 use apiKeySet:true so they always land on the Chat view.

test("first run with no API key opens the onboarding wizard", async ({ page }) => {
  await installMock(page, { apiKeySet: false, initialView: "onboarding" });
  await page.goto("/");
  // New routing: wizard overlay opens when no key is set
  await expect(page.getByTestId("onboard-wizard")).toBeVisible({ timeout: 5000 });
  // The wizard step bar must be shown (screen 1)
  await expect(page.getByTestId("onboard-step-bar")).toBeVisible();
});

test("saving an API key clears the gate and enables chat", async ({ page }) => {
  // Start with no key but routed to chat (wizard skipped via initialView override)
  // so we can test the Settings-based key-save flow in isolation
  await installMock(page, { apiKeySet: false, initialView: "chat" });
  await page.goto("/");
  // Since no wizard, the app routes to chat (api_key_set false → banner shown)
  await expect(page.getByTestId("apikey-banner")).toBeVisible({ timeout: 5000 });

  // Navigate to Settings, save a key
  await page.getByTestId("nav-settings").click();
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
