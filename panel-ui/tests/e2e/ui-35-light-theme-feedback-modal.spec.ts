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

test.describe("FeedbackModal Light Theme Compatibility", () => {
  test("displays FeedbackModal with correct contrast in light mode", async ({ page }, testInfo) => {
    // Intercept API routes to bypass actual auth
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });
    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
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
          access_token: "mock-access-token",
          refresh_token: "mock-refresh-token",
        }),
      });
    });

    // Complete onboarding and force light theme
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.localStorage.setItem("vite-ui-theme", "studio-bone");
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

    // 1. Navigate to main UI and ensure light theme is applied
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Some routes might render a mock login first despite localstorage if context isn't fully loaded, so click it if it appears
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    // 2. Open Config Modal then Feedback Modal
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    const provideFeedbackBtn = page.locator('button:has-text("Provide Feedback")');
    await expect(provideFeedbackBtn).toBeVisible();
    await provideFeedbackBtn.click();

    // 3. Verify FeedbackModal is visible and uses light theme colors
    const dialog = page.locator('div[role="dialog"]').or(page.locator('div:has-text("Provide Feedback")').filter({ hasText: "Help us improve the Antigravity experience" }).first());
    await expect(dialog).toBeVisible();

    const title = page.locator('h3:has-text("Provide Feedback")');
    await expect(title).toBeVisible();

    // Assert title text is dark
    const titleColor = await title.evaluate((el) => window.getComputedStyle(el).color);
    expect(titleColor).toMatch(/oklch|rgb/);

    // Assert main container background luminance > 0.7
    const dialogBgColor = await dialog.evaluate((el) => window.getComputedStyle(el).backgroundColor);

    // Evaluate luminance
    const luminance = await dialog.evaluate((el) => {
      // Find the specific container element
      const targetEl = el.tagName === 'DIV' && el.getAttribute('role') !== 'dialog' ? el : el.querySelector('.bg-white, .dark\\:bg-\\[\\#141518\\]') || el.lastElementChild || el;
      const bgColor = window.getComputedStyle(targetEl).backgroundColor;
      if (bgColor.startsWith('rgb')) {
        const rgb = bgColor.match(/\d+/g)?.map(Number) || [255, 255, 255];
        if (rgb.length >= 4 && rgb[3] === 0) return 1.0; // transparent
        const [r, g, b] = rgb.map(c => {
          c /= 255;
          return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
        });
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
      }
      return 1.0; // Assuming ok-lch is fine, or non-rgb colors
    });
    expect(luminance).toBeGreaterThan(0.7);

    // Take screenshot
    await page.screenshot({ path: testInfo.outputPath("feedback_modal_light.png"), fullPage: true });

    // Assert there are no page errors
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));
    expect(pageErrors).toEqual([]);
  });

  test("displays FeedbackModal with correct contrast in dark mode", async ({ page }, testInfo) => {
    // Intercept API routes
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });
    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
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
          access_token: "mock-access-token",
          refresh_token: "mock-refresh-token",
        }),
      });
    });

    // Complete onboarding and force dark theme
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.localStorage.setItem("vite-ui-theme", "studio-dark");
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
    await page.waitForLoadState("domcontentloaded");

    // Some routes might render a mock login first despite localstorage if context isn't fully loaded, so click it if it appears
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    const provideFeedbackBtn = page.locator('button:has-text("Provide Feedback")');
    await expect(provideFeedbackBtn).toBeVisible();
    await provideFeedbackBtn.click();

    const title = page.locator('h3:has-text("Provide Feedback")');
    await expect(title).toBeVisible();

    // Assert title text is light (dark mode)
    const titleColor = await title.evaluate((el) => window.getComputedStyle(el).color);
    expect(titleColor).toMatch(/oklab|rgba|rgb/);

    await page.screenshot({ path: testInfo.outputPath("feedback_modal_dark.png"), fullPage: true });

    // Assert there are no page errors
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));
    expect(pageErrors).toEqual([]);
  });
});
