const { defineConfig, devices } = require("@playwright/test");

const PORT = process.env.PORT || 5599;

// UI tests run against the static frontend in Chromium with an injected mock
// backend (window.__MOCK_BACKEND__). This covers frontend logic on every OS,
// including macOS where no native WKWebView WebDriver exists. Native smoke
// tests via tauri-driver are a separate, Windows/Linux-only harness.
module.exports = defineConfig({
  testDir: ".",
  timeout: 15000,
  use: {
    baseURL: `http://localhost:${PORT}`,
    headless: true,
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "node static-server.cjs",
    url: `http://localhost:${PORT}`,
    reuseExistingServer: true,
    timeout: 10000,
  },
});
