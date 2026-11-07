import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const DEFAULT_ARTIFACT_DIR = path.join(process.cwd(), "test-results", "artifacts");

function getWritableDir(target: string): string {
  try {
    fs.mkdirSync(target, { recursive: true });
    fs.accessSync(target, fs.constants.W_OK);
    return target;
  } catch {
    fs.mkdirSync(DEFAULT_ARTIFACT_DIR, { recursive: true });
    return DEFAULT_ARTIFACT_DIR;
  }
}

const ARTIFACT_DIR = getWritableDir(process.env.ARTIFACT_DIR || DEFAULT_ARTIFACT_DIR);

test.describe("Providers Light Theme E2E Verification", () => {
  test("Settings Providers page respects light theme and dark theme", async ({ page }, testInfo) => {
    // Intercept health and auth requests
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
      });
    });

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

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock.jwt.access-token",
          refresh_token: "mock-refresh-token",
        }),
      });
    });

    // We can also mock the provider APIs so it doesn't spin
    await page.route("**/panel/api/system/scan", async (route) => {
      await route.fulfill({ json: { providers: [] } });
    });
    await page.route("**/panel/api/providers/quota", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/panel/api/providers/config", async (route) => {
      await route.fulfill({ json: { providers: [] } });
    });

    // Bypass onboarding and set authenticated session with LIGHT theme seeded
    await page.addInitScript(() => {
      window.localStorage.setItem("vite-ui-theme", "light");
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

    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    // 255. Navigate to the app root
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // 2. Open Config Modal to inspect Settings
    const brainButton = page.locator('button[aria-label="Open Control Node"]').first();
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    // Wait for ConfigModal to appear
    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(modal).toBeVisible();

    // 3. Navigate to Providers
    const providersBtn = page.locator('button:has-text("Providers")').first();
    await providersBtn.click();

    // Verify the Providers view is visible
    const heading = page.locator('h2:has-text("AI Provider Ecosystem")');
    await expect(heading).toBeVisible();

    // 4. Assert Light Mode styling
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const primaryRoutingSection = page.locator('section').filter({ hasText: 'Primary Routing' }).first();

    // Light mode section background should have high luminance (surface-card #ffffff or similar)
    const bgLuminanceObj = await primaryRoutingSection.evaluate((el) => {
            const rgbaMatch = window.getComputedStyle(el).backgroundColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
      if (!rgbaMatch) return { luma: 0 };
      const r = parseInt(rgbaMatch[1], 10);
      const g = parseInt(rgbaMatch[2], 10);
      const b = parseInt(rgbaMatch[3], 10);
      return { luma: (0.299 * r + 0.587 * g + 0.114 * b) / 255 };
    });
    expect(bgLuminanceObj.luma).toBeGreaterThan(0.7);

    // Verify a heading or label is dark
    const headingColorObj = await heading.evaluate((el) => {
      const rgbaMatch = window.getComputedStyle(el).color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
      if (!rgbaMatch) return { luma: 1 };
      const r = parseInt(rgbaMatch[1], 10);
      const g = parseInt(rgbaMatch[2], 10);
      const b = parseInt(rgbaMatch[3], 10);
      return { luma: (0.299 * r + 0.587 * g + 0.114 * b) / 255 };
    });
    expect(headingColorObj.luma).toBeLessThan(0.3);

    // Take screenshot in Light Mode
    await page.screenshot({ path: testInfo.outputPath("providers_light_e2e.png"), fullPage: true });

    // 5. Switch back to Dark mode (using Appearance tab) to verify dark mode hasn't broken
    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await moonBtn.click();

    // Go back to Providers tab
    await providersBtn.click();
    await expect(heading).toBeVisible();

    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    // Dark mode section background should have low luminance
    const darkBgLuminanceObj = await primaryRoutingSection.evaluate((el) => {
            const rgbaMatch = window.getComputedStyle(el).backgroundColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
      if (!rgbaMatch) return { luma: 0 };
      const r = parseInt(rgbaMatch[1], 10);
      const g = parseInt(rgbaMatch[2], 10);
      const b = parseInt(rgbaMatch[3], 10);
      return { luma: (0.299 * r + 0.587 * g + 0.114 * b) / 255 };
    });
    expect(darkBgLuminanceObj.luma).toBeLessThan(0.3);

    // Take screenshot in Dark Mode
    await page.screenshot({ path: testInfo.outputPath("providers_dark_e2e.png"), fullPage: true });

    expect(pageErrors).toEqual([]);
  });
});
