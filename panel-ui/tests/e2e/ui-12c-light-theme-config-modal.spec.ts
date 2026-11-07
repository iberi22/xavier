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

test.describe("ConfigModal Light Theme Compliance", () => {
  test("adapts to light mode and displays correct luminance", async ({ page }, testInfo) => {
    // Mock endpoints required for authentication and rendering
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
          access_token: "mock-jwt-token",
          refresh_token: "mock-rt-token",
        }),
      });
    });

    // Seed light theme and authenticated state
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.sessionStorage.setItem("xavier.rt", "mock-rt-token");
      window.localStorage.setItem("vite-ui-theme", "light");
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

    // Capture errors
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    // Navigate to app and wait for load
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    const settingsButton = page.locator('button[aria-label="Open Control Node"], button[title="Control Node"]');

    // If login is shown, log in (auto-waiting handles the timing)
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible().catch(() => false)) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    // Wait for settings button to be ready and click
    await expect(settingsButton.first()).toBeVisible();
    await settingsButton.first().click();

    // Wait for ConfigModal to appear
    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(modal).toBeVisible();

    // Take screenshot of the opened modal in light mode
    const screenshotPath = testInfo.outputPath("config_modal_light.png");
    await page.screenshot({ path: screenshotPath, fullPage: true });

    // Verify html has studio-bone data-theme
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    // Check luminance of the main content area (which used to be dark only)
    const appearanceHeader = page.locator('h1:has-text("Appearance")');
    await expect(appearanceHeader).toBeVisible();

    const container = appearanceHeader.locator('xpath=ancestor::div[contains(@class, "flex-1 flex flex-col")]').first();
    const bgLuminance = await container.evaluate((el) => {
      const bg = window.getComputedStyle(el).backgroundColor;
      const rgb = bg.match(/\d+/g);
      if (!rgb) return 0;
      const [r, g, b] = rgb.map(Number);
      const a = [r, g, b].map(function (v) {
        v /= 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
    });

    // Ensure it's a light background (luminance > 0.7)
    expect(bgLuminance).toBeGreaterThan(0.7);

    // Let's verify controls in Appearance tab instead
    // Find and check internal controls like the light/dark mode buttons
    const appearanceControls = page.locator('button[aria-label="Light theme"], button[aria-label="Dark theme"]');
    await expect(appearanceControls).toHaveCount(2);

    const lightThemeBtn = page.locator('button[aria-label="Light theme"]');
    const lightBtnBg = await lightThemeBtn.evaluate((el) => {
      return window.getComputedStyle(el).backgroundColor;
    });
    // In light mode, the light theme button should be selected (it has bg-black/10 or similar)
    expect(lightBtnBg).not.toBe("rgba(0, 0, 0, 0)"); // Should have some background

    // Verify dark mode is still intact
    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await expect(moonBtn).toBeVisible();
    await moonBtn.click();

    // Verify html has studio-dark data-theme
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    const darkScreenshotPath = testInfo.outputPath("config_modal_dark.png");
    await page.screenshot({ path: darkScreenshotPath, fullPage: true });

    // Close modal
    await modal.click();
    await expect(modal).toBeHidden();

    expect(pageErrors).toEqual([]);
  });
});
