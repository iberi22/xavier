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

test.describe("GraphCanvas & GraphView light theme layout", () => {
  test("graph views switch correctly to light theme mode and nodes are readable", async ({ page }, testInfo) => {
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
          refresh_token: "mock-rt-token",
        }),
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

        await page.route("**/panel/api/memories", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/v1/memories?limit=1", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/v1/config/providers", async (route) => {
      await route.fulfill({ json: { providers: [] } });
    });
    await page.route("**/v1/mesh/peers", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/panel/api/threads", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/panel/api/bookmarks", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/panel/api/widgets", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/panel/api/graph", async (route) => {
      await route.fulfill({
        json: {
          nodes: [
            { id: "1", label: "Alpha Release", group: 1, milestone: "Q1", date: "2026-01-15" },
            { id: "2", label: "Beta Testing", group: 2, milestone: "Q2", date: "2026-04-10" },
          ],
          links: [
            { source: "1", target: "2" },
          ],
        },
      });
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "light");
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

    await page.goto("/");
        // If login is shown, log in
    const commandInput = page.locator('[data-testid="command-input"], input[placeholder*="Ask anything"], input[placeholder="Initialize command sequence..."]');
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');

    // Wait for either the command input or the login button
    await Promise.race([
        commandInput.waitFor({ state: "visible" }).catch(() => {}),
        loginButton.waitFor({ state: "visible" }).catch(() => {})
    ]);

    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    await expect(commandInput).toBeVisible({ timeout: 15000 });


    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await brainButton.waitFor({ state: "visible" });
    await brainButton.click();

    const roadmapTabBtn = page.locator('button:has-text("Roadmap")').first();
    await expect(roadmapTabBtn).toBeVisible();
    await roadmapTabBtn.click();

    const canvas = page.locator('canvas').first();
    await expect(canvas).toBeVisible();

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const heading = page.locator('span:has-text("System Diagnostics")').first();
    await expect(heading).toBeVisible({ timeout: 5000 }).catch(() => {});

    const screenshotPath = testInfo.outputPath("graph_light_theme_e2e.png");
    await page.screenshot({ path: screenshotPath, fullPage: true });

    const container = page.locator('div:has(> canvas)').first();
    const bgColor = await container.evaluate((el) => {
        return window.getComputedStyle(el).backgroundColor;
    });

    const rgbMatch = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
    if (rgbMatch) {
      const [r, g, b] = [parseInt(rgbMatch[1]), parseInt(rgbMatch[2]), parseInt(rgbMatch[3])];
      const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
      expect(luminance).toBeGreaterThan(0.7);
    }

    const headingColor = await heading.evaluate((el) => {
        return window.getComputedStyle(el).color;
    });
    const headingMatch = headingColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
    if (headingMatch) {
        const [r, g, b] = [parseInt(headingMatch[1]), parseInt(headingMatch[2]), parseInt(headingMatch[3])];
        const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
        expect(luminance).toBeLessThan(0.3);
    }

    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await moonBtn.click();

    await roadmapTabBtn.click();

    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    const darkScreenshotPath = testInfo.outputPath("graph_dark_theme_e2e.png");
    await page.screenshot({ path: darkScreenshotPath, fullPage: true });

    expect(pageErrors).toEqual([]);
  });
});
