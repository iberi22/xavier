import { expect, test } from "@playwright/test";

test.describe("Sovereign Wallet & Autonomous Agent Delegation E2E", () => {
  test.beforeEach(async ({ page }) => {
    // Mock Genesis Node endpoints
    await page.route("**/v1/auth/challenge", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ok: true,
          challenge_id: "test-challenge-uuid-wallet",
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
          token: "swal_sess_mock_wallet_token_777",
          node_id: "node_ed0fe57d1f71cbf6",
          karma: 25,
          genesis_node: "xaviercloud.swal.network",
          genesis_status: "connected",
          role: "node_peer",
        }),
      });
    });

    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online", service: "xaviercloud" } });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    // Seed local storage with authenticated session and skip onboarding
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_token", "swal_sess_mock_wallet_token_777");
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("swal_agent_delegation", "true");
      window.localStorage.setItem(
        "swal_node_session_v1",
        JSON.stringify({
          nodeId: "node_ed0fe57d1f71cbf6",
          token: "swal_sess_mock_wallet_token_777",
          karma: 25,
          genesisNode: "xaviercloud.swal.network",
          genesisStatus: "connected",
          connectedAt: Date.now(),
        })
      );
    });
  });

  test("opens Sovereign Wallet from TopStatusBar Karma pill and displays balances", async ({ page }) => {
    await page.goto("/");

    // TopStatusBar Karma pill should be visible
    const karmaPill = page.locator("button[title*='Billetera Soberana']");
    await expect(karmaPill).toBeVisible({ timeout: 10000 });
    await expect(karmaPill).toContainText("KARMA");

    // Click to open Wallet
    await karmaPill.click();

    // Verify Wallet Modal opens
    const modalTitle = page.locator("h2:has-text('Billetera Soberana SWAL')");
    await expect(modalTitle).toBeVisible({ timeout: 5000 });

    // Verify Balances (Layer 0 Karma and Layer 1 Polygon)
    await expect(page.getByText("Karma Acumulado", { exact: true })).toBeVisible();
    await expect(page.locator("text=Capa 1 (Polygon PoS)")).toBeVisible();
    await expect(page.locator("text=xaviercloud.swal.network")).toBeVisible();

    // Close modal via close button
    const closeBtn = page.locator("button[aria-label='Cerrar Billetera']");
    await expect(closeBtn).toBeVisible();
    await closeBtn.click();

    // Verify Modal closed
    await expect(modalTitle).not.toBeVisible();
  });

  test("configures Autonomous Agent Delegation limits and settings", async ({ page }) => {
    await page.goto("/#/wallet");

    // Verify Wallet is opened via direct hash route
    await expect(page.locator("h2:has-text('Billetera Soberana SWAL')")).toBeVisible({ timeout: 10000 });

    // Check Agent Delegation section is present
    await expect(page.locator("h3:has-text('Gestión Autónoma Delegada al Agente')")).toBeVisible();

    // Change daily limit
    const limitInput = page.locator("input[type='number']");
    await expect(limitInput).toBeVisible();
    await limitInput.fill("120");

    // Verify local storage updated
    const savedLimit = await page.evaluate(() => window.localStorage.getItem("swal_agent_daily_limit"));
    expect(savedLimit).toBe("120");

    // Verify Session Key and Knox/TPM security badges
    await expect(page.locator("text=Clave de Sesión efímera aislada en memoria")).toBeVisible();
    await expect(page.locator("text=Llave raíz Knox / TPM 100% protegida contra drenado")).toBeVisible();
  });

  test("saves Polygon payout address and executes Merkle rollup sync", async ({ page }) => {
    await page.goto("/#/wallet");

    await expect(page.locator("h2:has-text('Billetera Soberana SWAL')")).toBeVisible({ timeout: 10000 });

    // Fill Polygon payout address
    const testAddress = "0x71C84167B3aF58D351475c879894Fe95f87b8304";
    const addressInput = page.locator("input[placeholder*='0x...']");
    await addressInput.fill(testAddress);

    // Click Guardar Dirección
    const saveBtn = page.locator("button:has-text('Guardar Dirección')");
    await saveBtn.click();

    // Verify feedback
    await expect(page.locator("text=✓ Guardada")).toBeVisible();
    const storedAddress = await page.evaluate(() => window.localStorage.getItem("swal_polygon_payout_addr"));
    expect(storedAddress).toBe(testAddress);

    // Trigger Polygon Sync
    const syncBtn = page.locator("button:has-text('Sincronizar Recompensas a Polygon')");
    await syncBtn.click();

    // Verify loading and success transition
    await expect(page.locator("text=¡Rollup Compilado con Éxito!")).toBeVisible({ timeout: 5000 });
  });
});
