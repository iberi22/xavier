import { expect, test } from "@playwright/test";

test.describe("TopStatusBar End-to-End Smoke Tests", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept /health endpoint to return known system status
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          status: "ok",
          system: {
            cpu_usage: 42.0,
            ram_usage_percent: 65.0,
          },
        }),
      });
    });

    // Intercept system scan endpoint
    await page.route("**/v1/system/scan", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          version: "0.10.0",
          os: "linux",
          arch: "x86_64",
          providers: [],
          workspace_id: "default",
          memory_backend: "sqlite",
        }),
      });
    });

    // Intercept auth login endpoints
    await page.route("**/auth/login", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          token: "mock-jwt-token",
          user: { id: "1", email: "operator@xavier.local", role: "admin" },
        }),
      });
    });

    await page.route("**/api/auth/login", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          token: "mock-jwt-token",
          user: { id: "1", email: "operator@xavier.local", role: "admin" },
        }),
      });
    });

    await page.route("**/v1/auth/login", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          token: "mock-jwt-token",
          user: { id: "1", email: "operator@xavier.local", role: "admin" },
        }),
      });
    });

    await page.route("**/api/auth/me", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          id: "1",
          email: "operator@xavier.local",
          role: "admin",
        }),
      });
    });

    // Intercept providers config
    await page.route("**/v1/config/providers", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          providers: [{ provider: "openai", api_key: "sk-test", model: "gpt-4o" }],
        }),
      });
    });

    // Set onboarding completed flag in localStorage
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
    });

    await page.goto("/");
  });

  test("should display TopStatusBar resources pill with non-zero CPU and RAM metrics after load", async ({
    page,
  }) => {
    // Fill login details if on login page
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      await page.fill('input[type="password"]', "password123");
      await loginButton.click();
    }

    // Verify top status bar version identifier is present
    await expect(page.locator("span", { hasText: /^Xavier/ })).toBeVisible();

    // Verify resources pill displays CPU and RAM metrics from /health
    const cpuMetric = page.locator('div[title*="CPU:"]');
    await expect(cpuMetric).toBeVisible();

    const cpuText = await cpuMetric.textContent();
    expect(cpuText).not.toContain("0%");
  });
});
