// Playwright tests for the "What I know about you" facts panel in the
// Memory view. Drives the real frontend against the shared mock backend
// (mock-helpers.cjs), which seeds two example facts and implements the
// four user-fact commands (list/add/update/delete).

"use strict";

const { test, expect } = require("@playwright/test");
const { installMock } = require("./mock-helpers.cjs");

async function openMemory(page) {
  await installMock(page, { apiKeySet: true, initialView: "chat" });
  await page.goto("/");
  await page.getByTestId("nav-memory").click();
  await expect(page.getByTestId("view-memory")).toBeVisible();
  // Facts list hydrates when the Memory view opens.
  await expect(page.getByTestId("facts-list")).toBeVisible({ timeout: 5000 });
}

test.describe("What I know about you (facts panel)", () => {
  test("panel renders seeded facts", async ({ page }) => {
    await openMemory(page);
    const list = page.getByTestId("facts-list");
    await expect(list.getByTestId("fact-item")).toHaveCount(2, { timeout: 5000 });
    await expect(list).toContainText(/called by their first name/);
    await expect(list).toContainText(/Rust desktop assistant project/);
    // Existing file browser remains intact.
    await expect(page.getByTestId("memory-list")).toContainText(/MEMORY\.md/);
    await expect(page.getByTestId("memory-content")).toBeVisible();
  });

  test("adding a fact appends it to the list", async ({ page }) => {
    await openMemory(page);
    await page.getByTestId("facts-add-input").fill("Lives in the Pacific timezone.");
    await page.getByTestId("facts-add-btn").click();

    const list = page.getByTestId("facts-list");
    await expect(list).toContainText(/Pacific timezone/, { timeout: 5000 });
    await expect(list.getByTestId("fact-item")).toHaveCount(3);

    // The add input is cleared after submitting.
    await expect(page.getByTestId("facts-add-input")).toHaveValue("");

    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "add_user_fact")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
    expect(calls[calls.length - 1].args.value).toBe("Lives in the Pacific timezone.");
  });

  test("Enter in the add input adds a fact", async ({ page }) => {
    await openMemory(page);
    const input = page.getByTestId("facts-add-input");
    await input.fill("Enjoys black coffee.");
    await input.press("Enter");
    await expect(page.getByTestId("facts-list")).toContainText(/black coffee/, {
      timeout: 5000,
    });
  });

  test("editing a fact changes its value", async ({ page }) => {
    await openMemory(page);
    const firstFact = page.getByTestId("fact-item").first();
    await firstFact.getByTestId("fact-edit").click();

    const editInput = firstFact.getByTestId("fact-edit-input");
    await expect(editInput).toBeVisible();
    await editInput.fill("Prefers to be addressed formally.");
    await firstFact.getByTestId("fact-edit-save").click();

    await expect(page.getByTestId("facts-list")).toContainText(
      /addressed formally/,
      { timeout: 5000 }
    );

    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "update_user_fact")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
    expect(calls[calls.length - 1].args.value).toBe("Prefers to be addressed formally.");
  });

  test("forgetting a fact removes it from the list", async ({ page }) => {
    await openMemory(page);
    page.on("dialog", (d) => d.accept());

    await expect(page.getByTestId("fact-item")).toHaveCount(2);
    const target = page
      .getByTestId("fact-item")
      .filter({ hasText: "Rust desktop assistant project" });
    await target.hover();
    await target.getByTestId("fact-forget").click();

    await expect(page.getByTestId("fact-item")).toHaveCount(1, { timeout: 5000 });
    await expect(page.getByTestId("facts-list")).not.toContainText(
      /Rust desktop assistant project/
    );

    const calls = await page.evaluate(() =>
      window.__MOCK_CALLS__.filter((c) => c.cmd === "delete_user_fact")
    );
    expect(calls.length).toBeGreaterThanOrEqual(1);
  });

  test("empty state shows when no facts remain", async ({ page }) => {
    await openMemory(page);
    page.on("dialog", (d) => d.accept());

    // Forget both seeded facts.
    let item = page.getByTestId("fact-item").first();
    await item.hover();
    await item.getByTestId("fact-forget").click();
    await expect(page.getByTestId("fact-item")).toHaveCount(1, { timeout: 5000 });

    item = page.getByTestId("fact-item").first();
    await item.hover();
    await item.getByTestId("fact-forget").click();

    await expect(page.getByTestId("facts-list")).toContainText(
      /Nothing remembered yet/i,
      { timeout: 5000 }
    );
  });
});
