import { test, expect } from "@playwright/test";
import path from "node:path";
import fs from "node:fs";

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

test.describe("MemoryBrowser Light Theme", () => {
  test.beforeEach(async ({ page }) => {
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "ok" } });
    });
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock-token",
          refresh_token: "mock-refresh",
        }),
      });
    });
    await page.route("**/memory/search?*", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "light");
      window.localStorage.setItem("xavier_token", "mock-token");
      window.localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            token: "mock-token",
            isAuthenticated: true,
            user: { id: "1", email: "operator@xavier.local", role: "admin" },
          },
          version: 0,
        })
      );
    });
  });

  function getLuminance(rgbStr: string) {
    const match = rgbStr.match(/\d+(\.\d+)?/g);
    if (!match || match.length < 3) return 0;
    const [r, g, b] = match.map(Number).map(c => {
      const v = c / 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }

  test("MemoryBrowser follows light theme when vite-ui-theme=light", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    await page.goto("/#/");
    await page.getByRole("button", { name: /Open Control Node/ }).click();
    await page.getByRole("button", { name: "Memory", exact: true }).click();

    // Assert it's visible
    const browserHeading = page.getByRole("heading", { name: "Memory Browser" });
    await expect(browserHeading).toBeVisible();

    // The MemoryBrowser root container (we can select it by some text or class, but it's the parent of the heading)
    const browserContainer = browserHeading.locator("xpath=./../../..");

    // Check data-theme
    await expect(page.locator("html")).toHaveAttribute("data-theme", "studio-bone");

    // Check luminance of the container
    const bgLuminance = await browserContainer.evaluate((el) => {
      const bg = window.getComputedStyle(el).backgroundColor;
      // return a string so we can parse it here or just parse it inside
      return bg;
    });


    const luma = getLuminance(bgLuminance);
    expect(luma).toBeGreaterThan(0.7); // Light background

    // Check heading is dark
    const textLuminance = await browserHeading.evaluate((el) => {
      return window.getComputedStyle(el).color;
    });
    expect(getLuminance(textLuminance)).toBeLessThan(0.3); // Dark text

    await page.screenshot({ path: testInfo.outputPath("memory_browser_light.png"), fullPage: true });

    expect(pageErrors).toEqual([]);
  });

  test("MemoryBrowser is dark in dark mode", async ({ page }, testInfo) => {
    await page.addInitScript(() => {
      window.localStorage.setItem("vite-ui-theme", "studio-dark");
    });
    await page.goto("/#/");
    await page.getByRole("button", { name: /Open Control Node/ }).click();
    await page.getByRole("button", { name: "Memory", exact: true }).click();

    const browserHeading = page.getByRole("heading", { name: "Memory Browser" });
    await expect(browserHeading).toBeVisible();
    const browserContainer = browserHeading.locator("xpath=./../../..");

    const bgLuminance = await browserContainer.evaluate((el) => {
      return window.getComputedStyle(el).backgroundColor;
    });


    const luma = getLuminance(bgLuminance);
    expect(luma).toBeLessThan(0.3); // Dark background

    await page.screenshot({ path: testInfo.outputPath("memory_browser_dark.png"), fullPage: true });
  });
});
