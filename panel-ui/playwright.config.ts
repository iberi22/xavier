import { defineConfig, devices } from "@playwright/test";

const baseURL = process.env.PANEL_UI_BASE_URL ?? "http://127.0.0.1:4174";

export default defineConfig({
  testDir: "./tests",
  testMatch: "*.spec.ts",
  timeout: 60_000,
  expect: {
    timeout: 15_000,
  },
  fullyParallel: false,
  reporter: [["list"]],
  use: {
    baseURL,
    trace: "on-first-retry",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: "pnpm run dev",
    url: "http://127.0.0.1:4174",
    reuseExistingServer: !process.env.CI,
    timeout: 120 * 1000,
  },
});
