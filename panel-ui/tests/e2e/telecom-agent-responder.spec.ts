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

test.describe("Telecom Agent Responder E2E", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept health and auth requests
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
      });
    });

    await page.route("**/v1/config/providers", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ has_openai: true, has_gemini: true }),
      });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ token: "mock-jwt-token" }),
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
      await route.fulfill({ json: { data: { nodes: [], links: [] } } });
    });

    // Bypass onboarding and set authenticated session
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
  });

  test("renders AgentResponderSelect, toggles auto-responder state, opens dropdown, and captures artifacts", async ({ page }) => {
    // Navigate to Agent Responder route
    await page.goto("/#/telecom/agent-responder");
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(500);

    // Verify initial component rendering
    const headerTitle = page.locator("text=Automated Cognitive Responder");
    await expect(headerTitle).toBeVisible();

    const activeBadge = page.locator("text=Active");
    await expect(activeBadge).toBeVisible();

    // Verify initial selected agent
    const defaultAgent = page.locator("text=Legal & Compliance Sentinel");
    await expect(defaultAgent).toBeVisible();

    // Capture initial configuration screenshot
    const initialConfigArtifact = path.join(ARTIFACT_DIR, "telecom_agent_responder_config_e2e.png");
    await page.screenshot({ path: initialConfigArtifact, fullPage: true });

    // Open dropdown to select another cognitive agent
    const triggerBtn = page.locator('button[aria-label="Select cognitive agent responder"]');
    await expect(triggerBtn).toBeVisible();
    await triggerBtn.click();

    // Check dropdown options
    const sentinelOption = page.locator("text=Sentinel Threat Monitor");
    await expect(sentinelOption).toBeVisible();
    await sentinelOption.click();

    // Verify updated selected agent
    const updatedAgent = page.locator("h4", { hasText: "Sentinel Threat Monitor" });
    await expect(updatedAgent).toBeVisible();

    // Toggle master switch
    const toggleSwitch = page.locator('button[role="switch"]');
    await expect(toggleSwitch).toBeVisible();
    await toggleSwitch.click();

    // Capture updated state screenshot
    const updatedStateArtifact = path.join(ARTIFACT_DIR, "telecom_agent_responder_active_e2e.png");
    await page.screenshot({ path: updatedStateArtifact, fullPage: true });
  });
});
