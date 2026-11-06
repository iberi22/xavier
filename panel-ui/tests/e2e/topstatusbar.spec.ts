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

    // Set onboarding completed flag and authenticated session in localStorage
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.sessionStorage.setItem("xavier.rt", "mock-rt-token");
      window.localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            token: "mock-jwt-token",
            isAuthenticated: true,
            user: { id: "1", email: "operator@xavier.local", role: "admin" },
          },
          version: 0,
        })
      );
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
      const otherMethodsBtn = page.locator('button:has-text("Otros métodos")');
      if (await otherMethodsBtn.isVisible()) {
        await otherMethodsBtn.click();
      }
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    // Verify top status bar version identifier is present
    await expect(page.locator("span", { hasText: /^Xavier/ })).toBeVisible();

    // Verify resources pill displays CPU and RAM metrics from /health
    const cpuMetric = page.locator('div[title*="CPU:"]');
    await expect(cpuMetric).toBeVisible();

    await expect
      .poll(async () => {
        const title = (await cpuMetric.getAttribute("title")) || "";
        const text = (await cpuMetric.textContent()) || "";
        return title.includes("42%") || !text.includes("0%");
      }, { timeout: 10_000 })
      .toBe(true);
  });

  test("mobile layout should not have horizontal overflow and overflow menu is accessible", async ({ page }, testInfo) => {
    // Fill login details if on login page
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const otherMethodsBtn = page.locator('button:has-text("Otros métodos")');
      if (await otherMethodsBtn.isVisible()) {
        await otherMethodsBtn.click();
      }
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    await page.setViewportSize({ width: 390, height: 844 });

    // Wait for the components to load
    await expect(page.locator("span", { hasText: /^Xavier/ })).toBeVisible();

    // Check overflow menu button is visible on mobile
    const moreBtn = page.locator('button[aria-label="More status"]');
    await expect(moreBtn).toBeVisible();

    // Verify there is no horizontal scroll
    const hasHorizontalOverflow = await page.evaluate(() => {
      return document.documentElement.scrollWidth > document.documentElement.clientWidth;
    });
    expect(hasHorizontalOverflow).toBe(false);

    await moreBtn.click();

    // Check one of the secondary pills is now visible in the dropdown
    const syncBtn = page.locator('button[aria-label="Memory Sync Control & Status"]');
    await expect(syncBtn).toBeVisible();

    // Capture screenshot using testInfo.outputPath
    const screenshotPath = testInfo.outputPath("mobile-topstatusbar.png");
    await page.screenshot({ path: screenshotPath });
  });
});
