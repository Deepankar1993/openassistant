// Playwright tests for the Memory, Skills, Status/Doctor panels
// and expanded Settings sections.
// Covers tasks 7.7–7.10 from add-desktop-onboarding-options/tasks.md.

"use strict";

const { test, expect } = require("@playwright/test");
const { installMock } = require("./mock-helpers.cjs");

// ──────────────────────────────────────────────────────────────────────────────
// Memory view
// ──────────────────────────────────────────────────────────────────────────────

test.describe("Memory view", () => {
  test("Memory nav opens Memory view", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-memory").click();
    await expect(page.getByTestId("view-memory")).toBeVisible();
  });

  test("MEMORY.md content is rendered in the textarea", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-memory").click();

    // The app loads MEMORY.md on mount and shows it in the content area.
    // Use toHaveValue for textarea (value property, not textContent).
    await expect(page.getByTestId("memory-content")).toHaveValue(
      /Test memory content for Playwright/,
      { timeout: 5000 }
    );
  });

  test("MEMORY.md is editable and Save calls write_memory_md", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-memory").click();

    // Wait for content to load (use toHaveValue for textarea)
    await expect(page.getByTestId("memory-content")).toHaveValue(
      /Test memory content/,
      { timeout: 5000 }
    );

    // Save button should be visible (MEMORY.md is editable)
    await expect(page.getByTestId("memory-save")).toBeVisible();

    // Edit the content
    const newContent = "# Updated Memory\n\nUpdated by Playwright.";
    await page.getByTestId("memory-content").fill(newContent);

    // Click Save
    await page.getByTestId("memory-save").click();

    // write_memory_md should have been called with new content
    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "write_memory_md")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
    expect(calls[calls.length - 1].args.content).toBe(newContent);
  });

  test("file list contains MEMORY.md and a daily note", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-memory").click();

    const list = page.getByTestId("memory-list");
    await expect(list).toContainText(/MEMORY\.md/, { timeout: 5000 });
    await expect(list).toContainText(/2026-06-03/);
  });

  test("clicking a daily note shows it as read-only (no Save button)", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-memory").click();

    // Wait for list to render
    await expect(page.getByTestId("memory-list")).toContainText(
      /2026-06-03/,
      { timeout: 5000 }
    );

    // Click the daily note
    await page
      .locator("#memory-list .file-item")
      .filter({ hasText: "2026-06-03" })
      .click();

    // The content area should show today note content (use toHaveValue for textarea)
    await expect(page.getByTestId("memory-content")).toHaveValue(
      /test notes/,
      { timeout: 5000 }
    );

    // Save button should be hidden for read-only daily notes
    await expect(page.getByTestId("memory-save")).toBeHidden();
  });

  test("memory search box filters the file list", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-memory").click();

    // Wait for list to be populated
    await expect(page.getByTestId("memory-list")).toContainText(/MEMORY\.md/, {
      timeout: 5000,
    });

    // Type something that only matches the daily note
    await page.getByTestId("memory-search").fill("2026");

    // After debounce (300ms) the list should only show the daily note
    await page.waitForTimeout(400);
    await expect(page.getByTestId("memory-list")).not.toContainText(
      /MEMORY\.md/
    );
    await expect(page.getByTestId("memory-list")).toContainText(/2026-06-03/);
  });
});

// ──────────────────────────────────────────────────────────────────────────────
// Skills view
// ──────────────────────────────────────────────────────────────────────────────

test.describe("Skills view", () => {
  test("Skills nav opens Skills view", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-skills").click();
    await expect(page.getByTestId("view-skills")).toBeVisible();
  });

  test("built-in skills are listed under the Built-in section header", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-skills").click();

    const list = page.getByTestId("skills-list");
    await expect(list).toContainText(/Built-in/, { timeout: 5000 });
    await expect(list).toContainText(/summarize/);
    await expect(list).toContainText(/code-review/);
    await expect(list).toContainText(/daily-plan/);
  });

  test("clicking a skill shows content via read_skill", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-skills").click();

    // Wait for the list
    await expect(page.getByTestId("skills-list")).toContainText(/summarize/, {
      timeout: 5000,
    });

    // Click the "summarize" skill
    await page
      .locator("#skills-list .file-item")
      .filter({ hasText: "summarize" })
      .first()
      .click();

    // Content pane should render the skill
    await expect(page.getByTestId("skills-content")).toContainText(
      /summarize/,
      { timeout: 5000 }
    );
    // read_skill must have been called
    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "read_skill")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
    expect(calls[calls.length - 1].args.name).toBe("summarize");
  });

  test("no Activate button is visible in the skill detail", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-skills").click();

    await expect(page.getByTestId("skills-list")).toContainText(/summarize/, {
      timeout: 5000,
    });
    await page
      .locator("#skills-list .file-item")
      .filter({ hasText: "summarize" })
      .first()
      .click();

    // There should be NO button with text "Activate" anywhere in the skill pane
    await expect(
      page.locator("#skills-content button, #skills-content a").filter({
        hasText: /activate/i,
      })
    ).toHaveCount(0);

    // The note about future availability should be shown instead
    await expect(page.getByTestId("skills-content")).toContainText(
      /future update/i
    );
  });

  test("New Skill button opens the modal", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-skills").click();

    await page.getByTestId("skills-new").click();
    await expect(page.getByTestId("skill-modal")).toBeVisible();
  });

  test("New Skill modal: fill form and submit calls create_skill", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-skills").click();

    await page.getByTestId("skills-new").click();
    await expect(page.getByTestId("skill-modal")).toBeVisible();

    await page.getByTestId("skill-modal-name").fill("my-playwright-skill");
    await page
      .getByTestId("skill-modal-content")
      .fill("# my-playwright-skill\n\nDo something cool.");
    await page.getByTestId("skill-modal-save").click();

    // Modal should close
    await expect(page.getByTestId("skill-modal")).toBeHidden({ timeout: 3000 });

    // create_skill must have been called
    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "create_skill")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
    expect(calls[calls.length - 1].args.name).toBe("my-playwright-skill");

    // New skill should now appear in the list
    await expect(page.getByTestId("skills-list")).toContainText(
      /my-playwright-skill/,
      { timeout: 3000 }
    );
  });

  test("New Skill modal Cancel closes without creating", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-skills").click();

    await page.getByTestId("skills-new").click();
    await expect(page.getByTestId("skill-modal")).toBeVisible();
    await page.getByTestId("skill-modal-cancel").click();
    await expect(page.getByTestId("skill-modal")).toBeHidden();

    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "create_skill")
    );
    expect(calls).toHaveLength(0);
  });
});

// ──────────────────────────────────────────────────────────────────────────────
// Status / Doctor view
// ──────────────────────────────────────────────────────────────────────────────

test.describe("Status view", () => {
  test("Status nav opens Status view", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-status").click();
    await expect(page.getByTestId("view-status")).toBeVisible();
  });

  test("status card shows model and memory_db_entries", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-status").click();

    const card = page.getByTestId("status-card");
    await expect(card).toBeVisible({ timeout: 5000 });

    // Model row should show the mocked model name
    await expect(card).toContainText(/openrouter\/owl-alpha/);
    // Memory DB entries row (mocked at 12)
    await expect(card).toContainText(/12/);
  });

  test("status card shows message count", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-status").click();

    // message_count is 0 (no messages sent)
    await expect(page.getByTestId("status-card")).toContainText(/Messages/);
  });

  test("Run Diagnostics calls run_doctor and renders 6 result rows", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-status").click();

    await page.getByTestId("doctor-run").click();

    // Wait for results to appear
    const results = page.getByTestId("doctor-results");
    await expect(results).toContainText(/Config/, { timeout: 5000 });
    await expect(results).toContainText(/Memory database/);
    await expect(results).toContainText(/Memory workspace/);
    await expect(results).toContainText(/Skills/);
    await expect(results).toContainText(/Gateway/);
    await expect(results).toContainText(/Vision/);

    // Exactly 6 rows (doctor-row class)
    const rows = await results.locator(".doctor-row").count();
    expect(rows).toBe(6);

    // run_doctor must have been called
    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "run_doctor")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
  });

  test("Run Setup Wizard link visible when api_key_set is false", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: false, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-status").click();

    await expect(page.getByTestId("status-card")).toBeVisible({ timeout: 5000 });
    await expect(page.getByTestId("status-rerun-wizard")).toBeVisible();
  });

  test("Run Setup Wizard link hidden when api_key_set is true", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-status").click();

    await expect(page.getByTestId("status-card")).toBeVisible({ timeout: 5000 });
    await expect(page.getByTestId("status-rerun-wizard")).toBeHidden();
  });

  test("Run Setup Wizard link opens the wizard overlay", async ({ page }) => {
    await installMock(page, { apiKeySet: false, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-status").click();

    await expect(page.getByTestId("status-rerun-wizard")).toBeVisible({
      timeout: 5000,
    });
    await page.getByTestId("status-rerun-wizard").click();
    await expect(page.getByTestId("onboard-wizard")).toBeVisible();
  });
});

// ──────────────────────────────────────────────────────────────────────────────
// Expanded Settings
// ──────────────────────────────────────────────────────────────────────────────

test.describe("Expanded Settings", () => {
  test("Settings nav opens the two-column settings layout", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();
    await expect(page.getByTestId("view-settings")).toBeVisible();
    // The settings-nav sidebar must be present
    await expect(page.getByTestId("settings-nav")).toBeVisible();
  });

  test("Model section is active by default and shows provider/model/api-key fields", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();

    // Model section should be active (not hidden)
    await expect(page.locator("#settings-section-model")).not.toHaveClass(
      /hidden/
    );
    await expect(page.getByTestId("settings-provider")).toBeVisible();
    await expect(page.getByTestId("settings-model")).toBeVisible();
    await expect(page.getByTestId("settings-api-key")).toBeVisible();
  });

  test("clicking General section nav shows General content", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();

    await page
      .locator(".settings-nav-item[data-section='general']")
      .click();
    await expect(page.locator("#settings-section-general")).not.toHaveClass(
      /hidden/
    );
    await expect(page.getByTestId("settings-user-name")).toBeVisible();
  });

  test("clicking Tools section nav shows tools toggle", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();

    await page
      .locator(".settings-nav-item[data-section='tools']")
      .click();
    await expect(page.locator("#settings-section-tools")).not.toHaveClass(
      /hidden/
    );
    await expect(page.getByTestId("settings-tools-toggle")).toBeVisible();
  });

  test("clicking Memory section nav shows Memory settings", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();

    await page
      .locator(".settings-nav-item[data-section='memory']")
      .click();
    await expect(page.getByTestId("settings-memory")).not.toHaveClass(
      /hidden/
    );
    await expect(page.getByTestId("settings-memory-max")).toBeVisible();
  });

  test("clicking Channels section shows gateway config and a readiness check", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();

    await page
      .locator(".settings-nav-item[data-section='channels']")
      .click();
    await expect(page.getByTestId("settings-channels")).not.toHaveClass(
      /hidden/
    );
    // The stale "experimental" callout is gone; real gateway config is present.
    await expect(page.locator("#settings-section-channels")).not.toContainText(
      /experimental/i
    );
    await expect(page.getByTestId("settings-webhook-host")).toBeVisible();
    await expect(page.getByTestId("settings-webhook-port")).toBeVisible();
    await expect(page.getByTestId("settings-dm-policy")).toBeVisible();

    // The readiness panel populates on demand.
    await page.getByTestId("gateway-check").click();
    await expect(page.getByTestId("gateway-readiness")).toContainText(
      /WebChat server/i
    );

    // The gateway can be started in-process and reports running status.
    await expect(page.getByTestId("gateway-start")).toBeVisible();
    await page.getByTestId("gateway-start").click();
    await expect(page.getByTestId("gateway-run-status")).toContainText(/running/i);
    await page.getByTestId("gateway-stop").click();
    await expect(page.getByTestId("gateway-run-status")).toContainText(/stopped/i);
  });

  test("clicking Advanced section shows Re-run Setup Wizard button and app version", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();

    await page
      .locator(".settings-nav-item[data-section='advanced']")
      .click();
    await expect(page.getByTestId("settings-advanced")).not.toHaveClass(
      /hidden/
    );
    await expect(page.getByTestId("settings-rerun-wizard")).toBeVisible();
    await expect(page.getByTestId("settings-app-version")).toContainText(
      /0\.1\.0/,
      { timeout: 3000 }
    );
  });

  test("Model section Save still works (existing testid settings-save)", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();

    // Change the model value
    await page.getByTestId("settings-model").fill("openai/gpt-4o");
    await page.getByTestId("settings-save").click();
    await expect(page.getByTestId("save-status")).toHaveText(/Saved/, {
      timeout: 5000,
    });

    // save_full_config must have been called (the new settings path uses that)
    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "save_full_config")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
  });

  test("Tools section save calls save_full_config and set_tools_enabled", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();

    await page
      .locator(".settings-nav-item[data-section='tools']")
      .click();

    // Toggle tools on
    await page.getByTestId("settings-tools-toggle").check();
    await page.getByTestId("settings-tools-save").click();

    await expect(page.locator("#save-tools-status")).toHaveText(/Saved/, {
      timeout: 5000,
    });

    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "set_tools_enabled")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
    expect(calls[calls.length - 1].args.enabled).toBe(true);
  });

  test("General section save stores user_name", async ({ page }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();
    await page
      .locator(".settings-nav-item[data-section='general']")
      .click();

    await page.getByTestId("settings-user-name").fill("Alice");
    await page.getByTestId("settings-general-save").click();
    await expect(page.locator("#save-general-status")).toHaveText(/Saved/, {
      timeout: 5000,
    });

    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "save_full_config")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
    const dto = calls[calls.length - 1].args.dto;
    expect(dto.user_name).toBe("Alice");
  });

  test("data_dir chip is populated from config in General section", async ({
    page,
  }) => {
    await installMock(page, { apiKeySet: true, initialView: "chat" });
    await page.goto("/");
    await page.getByTestId("nav-settings").click();
    await page
      .locator(".settings-nav-item[data-section='general']")
      .click();

    await expect(page.getByTestId("settings-data-dir")).not.toBeEmpty({
      timeout: 3000,
    });
  });
});
