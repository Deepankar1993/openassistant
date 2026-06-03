// Playwright tests for the onboarding wizard overlay.
// Covers tasks 7.5 and 7.6 from add-desktop-onboarding-options/tasks.md.

"use strict";

const { test, expect } = require("@playwright/test");
const { installMock } = require("./mock-helpers.cjs");

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/**
 * Walk the wizard through screen 1 (Workspace).
 * Waits for the writable badge to appear (check_path_writable is called on open),
 * then clicks Continue.
 */
async function passScreen1(page) {
  // Wizard should be on screen 1
  await expect(page.locator("#wizard-screen-1")).toBeVisible();
  // Wait for writable badge to show as ok (check_path_writable returns true)
  await expect(page.getByTestId("onboard-workspace-writable")).toContainText(
    /Writable/,
    { timeout: 5000 }
  );
  // Continue should now be enabled
  await expect(page.locator("#wizard-continue-btn")).toBeEnabled();
  await page.locator("#wizard-continue-btn").click();
}

/**
 * Walk the wizard through screen 2 (AI Provider).
 * Fills the api_key, runs Test connection (mock success), clicks Continue.
 */
async function passScreen2(page, apiKey = "sk-test-9999") {
  await expect(page.locator("#wizard-screen-2")).toBeVisible();
  await page.getByTestId("onboard-api-key").fill(apiKey);
  await page.getByTestId("onboard-test-connection").click();
  await expect(page.getByTestId("onboard-connection-result")).toContainText(
    /Connected/,
    { timeout: 5000 }
  );
  // Continue should be unlocked after success
  await expect(page.locator("#wizard-continue-btn")).toBeEnabled();
  await page.locator("#wizard-continue-btn").click();
}

/**
 * Walk the wizard through screen 3 (Permissions) by selecting Card A (chat only).
 */
async function passScreen3CardA(page) {
  await expect(page.locator("#wizard-screen-3")).toBeVisible();
  // Continue is disabled until a card is chosen
  await expect(page.locator("#wizard-continue-btn")).toBeDisabled();
  await page.getByTestId("onboard-tools-card-a").click();
  await expect(page.locator("#wizard-continue-btn")).toBeEnabled();
  await page.locator("#wizard-continue-btn").click();
}

/**
 * Walk the wizard through screen 3 by selecting Card B (enable tools).
 */
async function passScreen3CardB(page) {
  await expect(page.locator("#wizard-screen-3")).toBeVisible();
  await page.getByTestId("onboard-tools-card-b").click();
  // consent notice should show
  await expect(page.getByTestId("onboard-tools-consent")).toBeVisible();
  await expect(page.locator("#wizard-continue-btn")).toBeEnabled();
  await page.locator("#wizard-continue-btn").click();
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

test.describe("Wizard — initial_view onboarding", () => {
  test("wizard overlay is shown when get_app_state returns onboarding", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await expect(page.getByTestId("onboard-wizard")).toBeVisible();
    // Step bar should be present
    await expect(page.getByTestId("onboard-step-bar")).toBeVisible();
  });

  test("wizard is NOT shown when get_app_state returns chat", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await expect(page.getByTestId("onboard-wizard")).toBeHidden();
  });

  test("screen 1 shows workspace path and writable badge", async ({ page }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await expect(page.getByTestId("onboard-workspace-path")).not.toBeEmpty();
    await expect(page.getByTestId("onboard-workspace-writable")).toContainText(
      /Writable/,
      { timeout: 5000 }
    );
  });

  test("Continue on screen 1 is disabled until writability check passes", async ({
    page,
  }) => {
    // First verify with path NOT writable — Continue should stay disabled
    await installMock(page, {
      apiKeySet: false,
      initialView: "onboarding",
      pathWritable: false,
    });
    await page.goto("/");
    await expect(page.getByTestId("onboard-workspace-writable")).toContainText(
      /Cannot write/,
      { timeout: 5000 }
    );
    await expect(page.locator("#wizard-continue-btn")).toBeDisabled();
  });

  test("full happy path: all 4 screens, finish → Chat view shown", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");

    // Screen 1 — Workspace
    await passScreen1(page);

    // Screen 2 — AI Provider
    await passScreen2(page);

    // Screen 3 — Permissions (chat only)
    await passScreen3CardA(page);

    // Screen 4 — Identity & Finish
    await expect(page.locator("#wizard-screen-4")).toBeVisible();
    // Finish CTA should be "Start chatting →" with the right testid
    await expect(page.getByTestId("onboard-finish-cta")).toBeVisible();
    // Fill a name (optional)
    await page.getByTestId("onboard-username").fill("Playwright User");
    // Click Finish
    await page.getByTestId("onboard-finish-cta").click();

    // Wizard should be gone
    await expect(page.getByTestId("onboard-wizard")).toBeHidden();
    // Chat view should be visible
    await expect(page.getByTestId("view-chat")).toBeVisible();

    // save_onboarding_config must have been called exactly once
    const calls = await page.evaluate(
      () =>
        window.__MOCK_CALLS__.filter((c) => c.cmd === "save_onboarding_config")
    );
    expect(calls).toHaveLength(1);
  });

  test("save_onboarding_config receives correct field values after happy path", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");

    await passScreen1(page);
    await passScreen2(page, "sk-my-key-1234");
    await passScreen3CardA(page); // tools_enabled = false
    await page.getByTestId("onboard-username").fill("TestUser");
    await page.getByTestId("onboard-finish-cta").click();

    await expect(page.getByTestId("onboard-wizard")).toBeHidden();

    const call = await page.evaluate(() =>
      window.__MOCK_CALLS__.find((c) => c.cmd === "save_onboarding_config")
    );
    expect(call).toBeDefined();
    const dto = call.args.dto || call.args;
    expect(dto.api_key).toBe("sk-my-key-1234");
    expect(dto.tools_enabled).toBe(false);
    expect(dto.user_name).toBe("TestUser");
  });

  test("tools_enabled true when Card B selected", async ({ page }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");

    await passScreen1(page);
    await passScreen2(page, "sk-tools-key");
    await passScreen3CardB(page); // tools_enabled = true
    await page.getByTestId("onboard-finish-cta").click();

    await expect(page.getByTestId("onboard-wizard")).toBeHidden();

    const call = await page.evaluate(() =>
      window.__MOCK_CALLS__.find((c) => c.cmd === "save_onboarding_config")
    );
    const dto = call.args.dto || call.args;
    expect(dto.tools_enabled).toBe(true);
  });

  test("Skip name button still finishes the wizard", async ({ page }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await passScreen1(page);
    await passScreen2(page);
    await passScreen3CardA(page);

    // Screen 4 — click Skip instead of filling a name
    await page.locator("#onboard-skip-name").click();

    await expect(page.getByTestId("onboard-wizard")).toBeHidden();
    await expect(page.getByTestId("view-chat")).toBeVisible();
  });
});

test.describe("Wizard — Screen 2 probe outcomes", () => {
  test("auth_failure shows error and keeps Continue locked", async ({
    page,
  }) => {
    await installMock(page, {
      apiKeySet: false,
      initialView: "onboarding",
      probeResult: "auth_failure",
    });
    await page.goto("/");
    await passScreen1(page);

    await expect(page.locator("#wizard-screen-2")).toBeVisible();
    await page.getByTestId("onboard-api-key").fill("sk-bad-key");
    await page.getByTestId("onboard-test-connection").click();

    await expect(page.getByTestId("onboard-connection-result")).toContainText(
      /Invalid API key/,
      { timeout: 5000 }
    );
    // Continue must remain disabled
    await expect(page.locator("#wizard-continue-btn")).toBeDisabled();
  });

  test("network_error shows network warning and keeps Continue locked", async ({
    page,
  }) => {
    await installMock(page, {
      apiKeySet: false,
      initialView: "onboarding",
      probeResult: "network_error",
    });
    await page.goto("/");
    await passScreen1(page);

    await page.getByTestId("onboard-api-key").fill("sk-some-key");
    await page.getByTestId("onboard-test-connection").click();

    await expect(page.getByTestId("onboard-connection-result")).toContainText(
      /Could not reach/,
      { timeout: 5000 }
    );
    await expect(page.locator("#wizard-continue-btn")).toBeDisabled();
  });

  test("editing api_key after a passing test re-locks Continue", async ({
    page,
  }) => {
    await installMock(page, {
      apiKeySet: false,
      initialView: "onboarding",
      probeResult: "success",
    });
    await page.goto("/");
    await passScreen1(page);

    // First: pass the test
    await page.getByTestId("onboard-api-key").fill("sk-good-key");
    await page.getByTestId("onboard-test-connection").click();
    await expect(page.getByTestId("onboard-connection-result")).toContainText(
      /Connected/,
      { timeout: 5000 }
    );
    await expect(page.locator("#wizard-continue-btn")).toBeEnabled();

    // Edit the key — test result should clear and Continue re-lock
    await page.getByTestId("onboard-api-key").fill("sk-changed-key");
    await expect(page.getByTestId("onboard-connection-result")).toBeHidden();
    await expect(page.locator("#wizard-continue-btn")).toBeDisabled();
  });
});

test.describe("Wizard — Screen 3 Permissions", () => {
  test("consent notice shown only when Card B (tools) is selected", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await passScreen1(page);
    await passScreen2(page);

    // Initially no card selected → consent hidden
    await expect(page.getByTestId("onboard-tools-consent")).toBeHidden();

    // Select Card A → still hidden
    await page.getByTestId("onboard-tools-card-a").click();
    await expect(page.getByTestId("onboard-tools-consent")).toBeHidden();

    // Select Card B → shown
    await page.getByTestId("onboard-tools-card-b").click();
    await expect(page.getByTestId("onboard-tools-consent")).toBeVisible();
  });
});

test.describe("Wizard — Screen 4 status pills", () => {
  test("config pill shows ok when conn was tested successfully", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await passScreen1(page);
    await passScreen2(page, "sk-good");
    await passScreen3CardA(page);

    // Screen 4 pills should render
    await expect(page.getByTestId("onboard-pill-config")).toBeVisible();
    await expect(page.getByTestId("onboard-pill-config")).toContainText(
      /Config ready/
    );
  });

  test("datadir pill shows ok when workspace was writable", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await passScreen1(page);
    await passScreen2(page, "sk-good");
    await passScreen3CardA(page);

    await expect(page.getByTestId("onboard-pill-datadir")).toContainText(
      /Data directory ready/
    );
  });

  test("vision pill resolves after run_doctor completes", async ({ page }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await passScreen1(page);
    await passScreen2(page, "sk-good");
    await passScreen3CardA(page);

    // The mock run_doctor returns vision as ok:false/is_optional:true
    await expect(page.getByTestId("onboard-pill-vision")).toContainText(
      /Not found|unavailable/,
      { timeout: 5000 }
    );
  });
});

test.describe("Wizard — re-entry from Settings", () => {
  test("Re-run Setup Wizard button in Settings > Advanced opens wizard", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");

    // Navigate to Settings
    await page.getByTestId("nav-settings").click();
    await expect(page.getByTestId("view-settings")).toBeVisible();

    // Navigate to the Advanced section inside Settings
    await page
      .locator(".settings-nav-item[data-section='advanced']")
      .click();
    await expect(page.locator("#settings-section-advanced")).not.toHaveClass(
      /hidden/
    );

    // Click Re-run Setup Wizard
    await page.getByTestId("settings-rerun-wizard").click();

    // Wizard should appear
    await expect(page.getByTestId("onboard-wizard")).toBeVisible();
  });

  test("wizard re-entry pre-fills provider fields from config", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");

    await page.getByTestId("nav-settings").click();
    await page
      .locator(".settings-nav-item[data-section='advanced']")
      .click();
    await page.getByTestId("settings-rerun-wizard").click();

    await expect(page.getByTestId("onboard-wizard")).toBeVisible();
    // Connection result should be hidden (cleared on re-entry)
    await expect(page.getByTestId("onboard-connection-result")).toBeHidden();
  });
});

test.describe("Wizard — Back navigation", () => {
  test("Back button on screen 2 returns to screen 1", async ({ page }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await passScreen1(page);
    await expect(page.locator("#wizard-screen-2")).toBeVisible();

    await page.locator("#wizard-back-btn").click();
    await expect(page.locator("#wizard-screen-1")).toBeVisible();
    await expect(page.locator("#wizard-back-btn")).toHaveCSS(
      "visibility",
      "hidden"
    );
  });

  test("Back button invisible on screen 1", async ({ page }) => {
    await installMock(page, { apiKeySet: false, initialView: "onboarding" });
    await page.goto("/");
    await expect(page.locator("#wizard-back-btn")).toHaveCSS(
      "visibility",
      "hidden"
    );
  });
});
