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

test.describe("Telecom Hub Rooms E2E", () => {
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

  test("renders TelecomHubView, navigates categories, sends optimistic message, and captures artifacts", async ({ page }) => {
    // Navigate to Telecom Hub route
    await page.goto("/#/telecom");
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(500);

    // Verify main header (src/components/Telecom/TelecomHubView.tsx)
    const hubHeader = page.locator("h1", { hasText: "Xavier Telecom Hub" });
    await expect(hubHeader).toBeVisible();

    // Verify channel tabs
    const directTab = page.locator('button[role="tab"]', { hasText: "Direct Chats" });
    const groupsTab = page.locator('button[role="tab"]', { hasText: "Group Rooms" });
    const peersTab = page.locator('button[role="tab"]', { hasText: "Network Peers" });

    await expect(directTab).toBeVisible();
    await expect(groupsTab).toBeVisible();
    await expect(peersTab).toBeVisible();

    // Verify initial selected room (MOCK_ROOMS' first "direct" room — initialTab defaults to "direct")
    const selectedTitle = page.locator("h2", { hasText: "node_alpha_8f (Primary Gateway)" });
    await expect(selectedTitle).toBeVisible();

    // Capture initial hub view
    const initialHubArtifact = path.join(ARTIFACT_DIR, "telecom_hub_rooms_e2e.png");
    await page.screenshot({ path: initialHubArtifact, fullPage: true });

    // Switch to Group Rooms Tab
    await groupsTab.click();
    const opsChannel = page.locator("button", { hasText: "Sovereign Mesh Operators" });
    await expect(opsChannel).toBeVisible();
    await opsChannel.click();

    // Verify active room switched to Sovereign Mesh Operators
    const activeGroupRoom = page.locator("h2", { hasText: "Sovereign Mesh Operators" });
    await expect(activeGroupRoom).toBeVisible();

    // Send an optimistic message in the room
    const messageInput = page.locator('input[placeholder*="Send encrypted message"]');
    await expect(messageInput).toBeVisible();
    await messageInput.fill("Telecom Hub Optimistic Dispatch Test");

    const sendBtn = page.locator('button[aria-label="Send Message"]');
    await expect(sendBtn).toBeEnabled();
    await sendBtn.click();

    // Assert optimistic message appeared in chat feed (scope to the message panel — the room
    // list item's lastMessage preview also renders this text, causing a strict-mode collision)
    const sentMsg = page
      .locator("section")
      .getByText("Telecom Hub Optimistic Dispatch Test");
    await expect(sentMsg).toBeVisible();

    // Switch to Peers Tab
    await peersTab.click();
    const peerNode = page.locator("button", { hasText: "node_delta_9e" });
    await expect(peerNode).toBeVisible();
    await peerNode.click();

    // Verify peer card in sidebar is visible
    const securitySidebar = page.locator("h3", { hasText: "Security & Key Info" });
    await expect(securitySidebar).toBeVisible();

    // Capture final state
    const finalStateArtifact = path.join(ARTIFACT_DIR, "telecom_hub_peers_e2e.png");
    await page.screenshot({ path: finalStateArtifact, fullPage: true });
  });
});
