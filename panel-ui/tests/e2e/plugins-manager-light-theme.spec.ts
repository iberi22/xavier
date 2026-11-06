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

test.describe("Plugins Manager Light Theme E2E", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept health requests
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
      });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock-jwt-token",
          refresh_token: "mock-refresh-token",
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

    // Intercept login requests
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

    // Mock initial state
    await page.route("**/panel/api/bookmarks", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/panel/api/widgets", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/panel/api/graph", async (route) => {
      await route.fulfill({ json: { data: { nodes: [], links: [] } } });
    });
    await page.route("**/panel/api/threads", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/v1/config/providers", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/notifications", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/notifications/stream**", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/v1/memories**", async (route) => {
      await route.fulfill({ json: [] });
    });

    // Intercept plugins API requests
    await page.route("**/plugins", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([
          {
            name: "codegraph",
            description: "Colby McHenry CodeGraph Engine",
            version: "0.1.1",
            installed: false,
            status: "Not Installed",
          },
          {
            name: "rtk-kernel",
            description: "RTK Kernel Proxy plugin",
            version: "1.0.0",
            installed: false,
            status: "Not Installed",
          },
        ]),
      });
    });

    // Intercept plugins install API
    await page.route("**/plugins/install", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "success" }),
      });
    });

    // Bypass onboarding and set authenticated session
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

    // Navigate to root
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Ensure session is initialized using expect for auto-waiting instead of sleep
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    const isLoginVisible = await loginButton.isVisible();
    if (isLoginVisible) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }
  });

  test("loads plugins manager in light theme and verifies luminosity", async ({ page }) => {

    // Switch to Light Mode via App Appearance Modal
    const configBtn = page.locator('button[aria-label="Open Control Node"]');
    await expect(configBtn).toBeVisible();
    await configBtn.click();

    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await expect(appearanceSidebarBtn).toBeVisible();
    await appearanceSidebarBtn.click();

    const sunBtn = page.locator('button[aria-label="Light theme"]');
    await expect(sunBtn).toBeVisible();
    await sunBtn.click();

    // Verify DOM theme attribute switched to studio-bone
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    // 2. Select Plugins tab
    const appSidebarBtn = page.getByTestId('settings-tab-plugins');
    await expect(appSidebarBtn).toBeVisible();
    await appSidebarBtn.click();

    // Verify Plugins Manager view is visible
    const pluginsHeader = page.getByText("Dynamic Plugin Ecosystem").first().or(page.locator('text="Dynamic Plugin Ecosystem"'));
    await expect(pluginsHeader).toBeVisible();

    // Wait for the plugin card container
    const pluginCard = page.locator('.p-5.rounded-2xl').first();
    await expect(pluginCard).toBeVisible();

    // 3. Verify Theme is set to studio-bone
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'studio-bone');

    // 4. Assert Container Luminance is > 0.7 in light theme
    const luminance = await pluginCard.evaluate((el) => {
      const style = window.getComputedStyle(el);
      const bg = style.backgroundColor; // e.g. "rgb(255, 255, 255)"

      const match = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return 0;

      const r = parseInt(match[1], 10);
      const g = parseInt(match[2], 10);
      const b = parseInt(match[3], 10);

      // Calculate relative luminance based on sRGB
      // using the formula: L = 0.299*R + 0.587*G + 0.114*B / 255
      const getL = (c: number) => {
        const cx = c / 255;
        return cx <= 0.03928 ? cx / 12.92 : Math.pow((cx + 0.055) / 1.055, 2.4);
      };

      return 0.2126 * getL(r) + 0.7152 * getL(g) + 0.0722 * getL(b);
    });

    expect(luminance).toBeGreaterThan(0.7);

    // 5. Capture Full-Page Screenshot
    const screenshotPath = path.join(ARTIFACT_DIR, "plugins_manager_light_theme_e2e.png");
    await page.screenshot({ path: screenshotPath, fullPage: true });

    // Verify file exists
    const fileExists = fs.existsSync(screenshotPath);
    expect(fileExists).toBeTruthy();
  });
});
