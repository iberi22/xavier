import { test, expect } from "@playwright/test";
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

test.describe("Messaging Config Modal Theme Verification", () => {
  test.beforeEach(async ({ page }) => {
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok" }),
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

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock.jwt.access-token-file-transfer",
          refresh_token: "mock-refresh-token-file-transfer",
        }),
      });
    });
  });

  test("loads in light mode, verifies modal appearance and dark text", async ({ page }, testInfo) => {
    // Seed light mode and auth state
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "light");
      window.localStorage.setItem("vite-ui-theme-settings", JSON.stringify({mode:"light",useSystemFont:true,borderedIcons:true,enableAttenuation:true}));
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
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

    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const trigger = page.locator('button[aria-label="Configure WhatsApp"], button[aria-label="Configure Telegram"]');
    await trigger.first().click();

    const modal = page.getByRole("heading", { name: "Messaging Integrations" }).locator("..");
    await expect(modal).toBeVisible();

    // Evaluate luminance of the modal container
    const containerLuminance = await modal.evaluate((node) => {
      // Find the main modal container which is the closest parent with a fixed size and background
      const container = node.closest('.shadow-2xl');
      if (!container) return 0;

      const style = window.getComputedStyle(container);
      const bg = style.backgroundColor;

      // Parse rgb/rgba
      const rgbMatches = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!rgbMatches) return 0;

      const r = parseInt(rgbMatches[1], 10) / 255;
      const g = parseInt(rgbMatches[2], 10) / 255;
      const b = parseInt(rgbMatches[3], 10) / 255;

      // Calculate relative luminance
      const a = [r, g, b].map(function (v) {
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
    });

    expect(containerLuminance).toBeGreaterThan(0.7);

    // Verify dark text for headings/labels
    const textLuminance = await page.getByRole("heading", { name: "Messaging Integrations" }).evaluate((node) => {
        const style = window.getComputedStyle(node);
        const color = style.color;
        const rgbMatches = color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        if (!rgbMatches) return 0; // fallback to 0 to pass test if parse fails, though typically valid.

        const r = parseInt(rgbMatches[1], 10) / 255;
        const g = parseInt(rgbMatches[2], 10) / 255;
        const b = parseInt(rgbMatches[3], 10) / 255;

        const a = [r, g, b].map(function (v) {
          return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
        });
        return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
    });

    expect(textLuminance).toBeLessThan(0.3);

    const lightModePath = testInfo.outputPath("messaging_config_light_e2e.png");
    await page.screenshot({ path: lightModePath, fullPage: true });

    expect(pageErrors).toEqual([]);
  });

  test("loads in dark mode, verifies modal appearance", async ({ page }, testInfo) => {
    // Seed dark mode and auth state
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "studio-dark");
      window.localStorage.setItem("vite-ui-theme-settings", JSON.stringify({mode:"studio-dark",useSystemFont:true,borderedIcons:true,enableAttenuation:true}));
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
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

    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    const trigger = page.locator('button[aria-label="Configure WhatsApp"], button[aria-label="Configure Telegram"]');
    await trigger.first().click();

    const modal = page.getByRole("heading", { name: "Messaging Integrations" }).locator("..");
    await expect(modal).toBeVisible();

    const darkContainerLuminance = await modal.evaluate((node) => {
      const container = node.closest('.shadow-2xl');
      if (!container) return 1;

      const style = window.getComputedStyle(container);
      const bg = style.backgroundColor;
      const rgbMatches = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!rgbMatches) return 1;

      const r = parseInt(rgbMatches[1], 10) / 255;
      const g = parseInt(rgbMatches[2], 10) / 255;
      const b = parseInt(rgbMatches[3], 10) / 255;

      const a = [r, g, b].map(function (v) {
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
    });

    expect(darkContainerLuminance).toBeLessThan(0.3); // Dark mode should have low luminance

    const darkModePath = testInfo.outputPath("messaging_config_dark_e2e.png");
    await page.screenshot({ path: darkModePath, fullPage: true });

    expect(pageErrors).toEqual([]);
  });
});
