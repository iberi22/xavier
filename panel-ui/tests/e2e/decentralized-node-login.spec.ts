import { expect, test } from "@playwright/test";

test.describe("Decentralized Node Login & Genesis Mesh E2E", () => {
  test("presents Sovereign Node Vault login option and connects to Genesis Node", async ({ page }) => {
    // Mock Genesis Node endpoints
    await page.route("**/v1/auth/challenge", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ok: true,
          challenge_id: "test-challenge-uuid-1234",
          nonce_hex: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
          issued_at: Math.floor(Date.now() / 1000),
          expires_at: Math.floor(Date.now() / 1000) + 60,
          domain: "swal-mesh-signed-nonce-v1",
          genesis_node: "xaviercloud.swal.network",
        }),
      });
    });

    await page.route("**/v1/auth/verify", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ok: true,
          token: "swal_sess_mock_genesis_token_999",
          node_id: "node_mock_e2e_node_123",
          karma: 10,
          genesis_node: "xaviercloud.swal.network",
          genesis_status: "connected",
          role: "peer_node",
        }),
      });
    });

    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online", service: "xaviercloud" } });
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

    // Clear prior storage to ensure we land on clean Login
    await page.addInitScript(() => {
      window.localStorage.removeItem("xavier_token");
      window.localStorage.removeItem("swal_node_session_v1");
      window.localStorage.setItem("xavier_onboarding_completed", "true");
    });

    await page.goto("/#/login");

    // Verify Title and Génesis Mesh badge
    await expect(page.locator("h1:has-text('XAVIER LOGIN')")).toBeVisible({ timeout: 10000 });
    await expect(page.locator("span:has-text('GÉNESIS MESH')")).toBeVisible();

    // Verify Sovereign Node card details
    await expect(page.locator("span:has-text('xaviercloud.swal.network')")).toBeVisible();
    await expect(page.locator("span:has-text('+10 Karma ($SWAL)')")).toBeVisible();

    // Click on CONECTAR NODO Y ENTRAR (VAULT)
    const nodeLoginBtn = page.locator("button:has-text('CONECTAR NODO Y ENTRAR (VAULT)')");
    await expect(nodeLoginBtn).toBeVisible();
    await nodeLoginBtn.click();

    // Verify transition into the main application
    await expect(page.locator("h1:has-text('XAVIER LOGIN')")).not.toBeVisible({ timeout: 10000 });

    // Verify TopStatusBar renders the Karma Pill
    const karmaBadge = page.locator("text=/\\d+ KARMA/");
    await expect(karmaBadge.first()).toBeVisible({ timeout: 10000 });
  });
});
