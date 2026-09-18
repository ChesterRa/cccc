import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/browser/gates",
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: "http://127.0.0.1:15559",
    viewport: { width: 1440, height: 1000 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    serviceWorkers: "block",
    launchOptions: process.env.CHROME_BIN ? { executablePath: process.env.CHROME_BIN } : {},
  },
  projects: [
    { name: "critical", testMatch: "critical.pw.js" },
    { name: "resources", testMatch: "resources.pw.js" },
    ...["en", "zh", "ja"].flatMap((locale) =>
      ["light", "dark"].map((theme) => ({
        name: `${locale}-${theme}`,
        testMatch: "layout.pw.js",
        metadata: { locale, theme },
      })),
    ),
  ],
  webServer: {
    command: "node tests/browser/gates/server.mjs",
    url: "http://127.0.0.1:15559/ui/tests/browser/group-work.html",
    reuseExistingServer: false,
    timeout: 120_000,
    gracefulShutdown: { signal: "SIGTERM", timeout: 5_000 },
  },
});
