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

test.describe("Telecom Direct Chat E2E", () => {
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

  test("renders DirectChatView, validates peer identity, sends message, and checks receipts", async ({ page }) => {
    // Navigate to Direct Chat route
    await page.goto("/#/telecom/direct");
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(500);

    // Verify peer identity & online status indicator (DirectChatView's DEFAULT_PEER, see
    // src/components/Telecom/DirectChatView.tsx)
    const peerHeading = page.locator("h2", { hasText: "Node Beta" });
    await expect(peerHeading).toBeVisible();

    const statusBadge = page.locator("text=Online · Direct Link Active");
    await expect(statusBadge).toBeVisible();

    const cipherBadge = page.locator("text=ChaCha20-Poly1305").first();
    await expect(cipherBadge).toBeVisible();

    // Verify existing pre-seeded message rendering (DirectChatView's DEFAULT_MESSAGES)
    const existingMsg = page.locator(
      "text=Encrypted 1-to-1 channel initialized via ChaCha20-Poly1305 key exchange."
    );
    await expect(existingMsg).toBeVisible();

    // Take screenshot of direct chat room
    const chatRoomArtifact = path.join(ARTIFACT_DIR, "telecom_direct_chat_e2e.png");
    await page.screenshot({ path: chatRoomArtifact, fullPage: true });

    // Type and send a new encrypted direct message
    const messageInput = page.locator('input[placeholder*="Send encrypted message"]');
    await expect(messageInput).toBeVisible();

    await messageInput.fill("E2E Test Encrypted Message with Receipt Verification");
    const sendButton = page.locator('button[aria-label="Send direct message"]');
    await expect(sendButton).toBeEnabled();
    await sendButton.click();

    // Verify outgoing message appears in stream
    const sentMsg = page.locator("text=E2E Test Encrypted Message with Receipt Verification");
    await expect(sentMsg).toBeVisible();

    // Verify delivery receipt indicator (DirectChatView.MessageItem renders
    // <span title="Sending...|Sent|Delivered|Read">, never "Status: ...")
    const receiptIndicator = page.locator(
      'span[title="Sending..."], span[title="Sent"], span[title="Delivered"], span[title="Read"]'
    );
    await expect(receiptIndicator.first()).toBeVisible();

    // Toggle Search conversation bar
    const searchBtn = page.locator('button[aria-label="Search conversation messages"]');
    await searchBtn.click();

    const searchInput = page.locator('input[placeholder="Search in encrypted chat..."]');
    await expect(searchInput).toBeVisible();
    await searchInput.fill("handshake");

    // Capture search filter state
    const searchArtifact = path.join(ARTIFACT_DIR, "telecom_direct_chat_search_e2e.png");
    await page.screenshot({ path: searchArtifact, fullPage: true });
  });
});
