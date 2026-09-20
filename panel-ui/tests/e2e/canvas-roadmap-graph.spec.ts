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

test.describe("GraphCanvas interactive roadmap navigation & node filtering", () => {
  test.beforeEach(async ({ page }) => {
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
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
            { id: "3", label: "Production Launch", group: 3, milestone: "Q3", date: "2026-07-20" },
          ],
          links: [
            { source: "1", target: "2" },
            { source: "2", target: "3" },
          ],
        },
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

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
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

    await page.goto("/");

    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      await page.fill('input[type="password"]', "password123");
      await loginButton.click();
    }

    await expect(
      page.locator('[data-testid="command-input"], input[placeholder*="Ask anything"], input[placeholder="Initialize command sequence..."]'),
    ).toBeVisible();
  });

  test("should open ConfigModal, switch to roadmap, interact with filters, and capture screenshot", async ({ page }) => {
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    const roadmapTabBtn = page.locator('button:has-text("Roadmap")').first();
    await expect(roadmapTabBtn).toBeVisible();
    await roadmapTabBtn.click();

    // Verify canvas is visible
    const canvas = page.locator('canvas').first();
    await expect(canvas).toBeVisible();

    // Wait for force graph to load initially
    await page.waitForTimeout(1000);

    // Filter by start date
    const startDateInput = page.locator('div:has(> label:has-text("Start Date")) > input[type="date"]');
    if (await startDateInput.isVisible()) {
        await startDateInput.fill("2026-02-01");
    }

    // Filter by milestone
    const milestoneSelect = page.locator('div:has(> label:has-text("Milestone")) > select');
    await milestoneSelect.waitFor({ state: 'visible' });
    const options = await milestoneSelect.locator('option').allInnerTexts();
    await milestoneSelect.selectOption(options.includes("Q3") ? "Q3" : options[options.length - 1]);

    await page.waitForTimeout(1000);

    const screenshotPath = path.join(ARTIFACT_DIR, "graph_canvas_e2e.png");
    await page.screenshot({ path: screenshotPath, fullPage: true });

    // Clear filters
    const clearBtn = page.locator('button[aria-label="Clear roadmap filters"]');
    if (await clearBtn.isVisible()) {
      await clearBtn.click();
    }
  });
});
