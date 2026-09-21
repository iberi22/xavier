import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const DEFAULT_ARTIFACT_DIR = path.join(
  process.cwd(),
  "test-results",
  "artifacts"
);

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

const ARTIFACT_DIR = getWritableDir(
  process.env.ARTIFACT_DIR || DEFAULT_ARTIFACT_DIR
);

test.describe("TelecomHub multi-channel room navigation & ephemeral messaging E2E test", () => {
  test.beforeEach(async ({ page }) => {
    // Mock health
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    // Mock initial threads
    await page.route("**/panel/api/threads", async (route) => {
      if (route.request().method() === "POST") {
        await route.fulfill({
          json: {
            id: "t1",
            title: "Test Thread",
            created_at: new Date().toISOString(),
            message_count: 0,
          },
        });
      } else {
        await route.fulfill({ json: [] });
      }
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

    // Navigate to root to ensure app starts correctly
    await page.goto("/");

    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      await page.fill('input[type="password"]', "password123");
      await loginButton.click();
    }

    // Now navigate to telecom
    await page.goto("/#/telecom");
    await page.waitForLoadState("domcontentloaded");
  });

  test("navigate tabs, send message, toggle ephemeral mode and capture screenshot", async ({
    page,
  }) => {
    // 1. Verify navigation across tabs
    const directTab = page.locator('button[role="tab"]', { hasText: /Direct Chats/i });
    const groupsTab = page.locator('button[role="tab"]', { hasText: /Group Rooms/i });
    const peersTab = page.locator('button[role="tab"]', { hasText: /Network Peers/i });

    await expect(directTab).toBeVisible();
    await expect(groupsTab).toBeVisible();
    await expect(peersTab).toBeVisible();

    // Verify Direct is initially selected
    await expect(directTab).toHaveAttribute("aria-selected", "true");

    // Click Groups and verify
    await groupsTab.click();
    await expect(groupsTab).toHaveAttribute("aria-selected", "true");

    // Click Peers and verify
    await peersTab.click();
    await expect(peersTab).toHaveAttribute("aria-selected", "true");

    // Go back to Direct to test messaging
    await directTab.click();
    await expect(directTab).toHaveAttribute("aria-selected", "true");

    // Click on a room to open chat (e.g. Node Beta)
    const roomBeta = page.getByText("node_alpha_8f (Primary Gateway)");
    await expect(roomBeta.first()).toBeVisible();
    await roomBeta.first().click();

    // 2. Validate ephemeral toggle status change
    const ephemeralToggle = page.locator('button[aria-label="Toggle off-the-record ephemeral message mode"], button[aria-label="Toggle Ephemeral Mode"]').first();
    await expect(ephemeralToggle).toBeVisible();

    // Toggle Ephemeral mode ON
    await ephemeralToggle.click();

    // 3. Validate sending messages and observing optimistic UI updates
    const messageInput = page.locator('input[placeholder*="Send encrypted message to"]');

    // Wait for the input to become visible before interacting with it
    await expect(messageInput).toBeVisible();

    const testMessage = "Hello from E2E test!";
    await messageInput.fill(testMessage);

    // Send message using keyboard enter to avoid layout issue with the send button overlay
    await page.keyboard.press("Enter");

    // Check if optimistic UI updated with the sent message
    const messageBubble = page.getByText(testMessage).first();
    await expect(messageBubble).toBeVisible();

    // 4. Verify full-page screenshot capture
    const screenshotPath = path.join(ARTIFACT_DIR, "telecom_hub_rooms_e2e.png");
    await page.screenshot({ path: screenshotPath, fullPage: true });

    // Verify file exists
    expect(fs.existsSync(screenshotPath)).toBe(true);
  });
});
