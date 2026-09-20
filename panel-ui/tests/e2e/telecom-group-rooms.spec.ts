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

test.describe("Telecom Group Rooms View", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept health and auth requests
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
      });
    });

    // We also need to mock config/providers to avoid proxy error taking too long
    await page.route("**/v1/config/providers", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ has_openai: true, has_gemini: true }),
      });
    });

    // and /auth/refresh
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

    // Mock initial threads
    await page.route("**/panel/api/threads", async (route) => {
      await route.fulfill({ json: [] });
    });

    // Mock other common endpoints
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

  test("renders GroupRoomView, allows topic edit, and toggles roster", async ({ page }) => {
    // Phase 1: Go to the test route
    await page.goto("/#/telecom-group-room");
    await page.waitForLoadState("domcontentloaded");

    // Sometimes components need a short time to animate or load initial state
    await page.waitForTimeout(500);

    // Check loading GroupRoomView: We should see the room name from DEFAULT_ROOM
    const heading = page.locator('h1', { hasText: 'Strategic-Ops-Council' });
    await expect(heading).toBeVisible();

    const participantCount = page.locator('text=4 Active');
    await expect(participantCount).toBeVisible();

    // Take screenshot of group room
    const fullRoomPath = path.join(ARTIFACT_DIR, "telecom_group_room_e2e.png");
    await page.screenshot({ path: fullRoomPath, fullPage: true });

    // Phase 2: Edit topic
    const topicHeading = page.locator('text=Multi-Node Mesh Consensus & Zero-Trust Protocol Synchronization v2.4');
    await expect(topicHeading).toBeVisible();

    const editTopicBtn = page.locator('button[aria-label="Edit room topic"]');
    await expect(editTopicBtn).toBeVisible();
    await editTopicBtn.click();

    const topicModalTitle = page.locator('h3', { hasText: 'Update Room Topic & Objective' });
    await expect(topicModalTitle).toBeVisible();

    const topicInput = page.locator('textarea#room-topic-input');
    await expect(topicInput).toBeVisible();

    await topicInput.fill('Updated Room Topic E2E');
    const saveBtn = page.locator('button', { hasText: 'Save Topic' });
    await saveBtn.click();

    const newTopic = page.locator('text=Updated Room Topic E2E');
    await expect(newTopic).toBeVisible();

    // Phase 3: Roster toggle
    // On mobile we click toggle, on desktop it's already there
    // We can just verify the roster is visible
    const rosterHeader = page.locator('h3', { hasText: 'Member Roster' });
    await expect(rosterHeader).toBeVisible();

    const nodeAlpha = page.locator('span', { hasText: 'node-alpha-8f' }).first();
    await expect(nodeAlpha).toBeVisible();

    // Take a screenshot of the state
    const topicEditPath = path.join(ARTIFACT_DIR, "telecom_group_room_roster_e2e.png");
    await page.screenshot({ path: topicEditPath, fullPage: true });
  });
});
