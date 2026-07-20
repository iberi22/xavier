import { expect, test } from "@playwright/test";

test.describe("Mesh and Data Commons Flow", () => {
  test.beforeEach(async ({ page }) => {
    // Mock health endpoint
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "online", version: "1.0.0" }),
      });
    });

    // Mock API responses
    await page.route("**/v1/mesh/peers", async (route) => {
      await route.fulfill({
        json: {
          local_node_id: "local-node-123",
          peers: [
            {
              node_id: "peer-node-456",
              alias: "Remote Xavier",
              endpoint_url: "http://remote-xavier:8006",
              role: "reader",
              clearance: "unclassified",
              last_seen_at: Date.now() / 1000 - 60,
              sync_enabled: true,
            },
          ],
        },
      });
    });

    await page.route("**/v1/mesh/cloud", async (route) => {
      await route.fulfill({
        json: {
          url: "https://xyz.supabase.co",
          token: "secret-token",
          instance_id: "test-instance",
          sync_interval_ms: 300000,
          auto_heartbeat: true,
        },
      });
    });

    await page.route("**/v1/mesh/data_commons/opt_in", async (route) => {
      await route.fulfill({
        json: {
          enabled: false,
          consent_given: false,
          wallet_address: "",
        },
      });
    });

    await page.route("**/notifications", async (route) => {
      await route.fulfill({
        json: [
          {
            id: "n1",
            island_id: "system",
            title: "System Update",
            body: "Xavier core has been updated.",
            timestamp: new Date().toISOString(),
            read: false,
            severity: "info",
          },
          {
            id: "n2",
            island_id: "memory",
            title: "New Memory",
            body: "A new project has been indexed.",
            timestamp: new Date().toISOString(),
            read: true,
            severity: "success",
          },
        ],
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

    await page.goto("/");
    await page.evaluate(() => {
      localStorage.setItem("xavier_onboarding_completed", "true");
    });
    await page.goto("/"); // Reload to apply localStorage
    await page.fill('input[placeholder="XAVIER_TOKEN"]', "test-token");
    await page.click('button:has-text("INITIALIZE SESSION")');
  });

  test("should manage mesh network settings", async ({ page }) => {
    // Open Mesh tab
    await page.click('button[title="Open Control Node"]');
    await page.click('button:has-text("Mesh")');

    await expect(page.getByText("local-node-123")).toBeVisible();
    await expect(page.getByText("Remote Xavier")).toBeVisible();

    // Generate pairing code
    await page.route("**/v1/mesh/peers/generate-code", async (route) => {
      await route.fulfill({
        json: { code: "ABC-123-XYZ", secret: "super-secret" },
      });
    });
    await page.click('button:has-text("Generate")');
    await expect(page.getByText("ABC-123-XYZ")).toBeVisible();
    await expect(page.getByText("super-secret")).toBeVisible();

    // Join mesh
    await page.route("**/v1/mesh/peers/pair", async (route) => {
      await route.fulfill({ json: { status: "ok", node_id: "new-node" } });
    });
    await page.fill(
      'input[placeholder="Paste code from another node"]',
      "PEER-CODE-999",
    );
    await page.click('button:has-text("Join")');

    // Update ACL
    await page.route("**/v1/mesh/peers/peer-node-456/acl", async (route) => {
      await route.fulfill({ json: { status: "ok" } });
    });
    await page.locator("select").first().selectOption("secret");
    await page.locator("select").last().selectOption("admin");
  });

  test("should configure cloud relay and data commons", async ({ page }) => {
    // Open Configuration -> Server & Network
    await page.click('button[title="Open Control Node"]');
    await page.click('button:has-text("Configuration")');
    await page.click('button:has-text("Server & Network")');

    // Cloud Relay
    await expect(
      page.locator('input[value="https://xyz.supabase.co"]'),
    ).toBeVisible();
    await page.fill(
      'input[placeholder="pgheart-namespace-id"]',
      "new-instance",
    );

    await page.route("**/v1/mesh/cloud", async (route) => {
      expect(route.request().method()).toBe("PUT");
      const body = route.request().postDataJSON();
      expect(body.instance_id).toBe("new-instance");
      await route.fulfill({ json: { status: "ok" } });
    });
    await page.click('button:has-text("Save Relay Config")');
    await expect(page.getByText("Settings applied")).toBeVisible();

    // Data Commons
    await expect(page.getByText("Xavier Data Commons")).toBeVisible();

    // Toggle opt-in
    // Find the section and then the specific toggle button
    await page
      .locator("div.flex.items-center.justify-between", {
        hasText: "Enable Data Telemetry",
      })
      .locator("button")
      .click();
    await page
      .locator("div.flex.items-center.justify-between", {
        hasText: "GDPR / Legal Consent",
      })
      .locator("button")
      .click();
    await page.fill(
      'input[placeholder^="e.g. HN7cABqLq46Es1jh92d"]',
      "my-wallet-address",
    );

    await page.route("**/v1/mesh/data_commons/opt_in", async (route) => {
      expect(route.request().method()).toBe("POST");
      const body = route.request().postDataJSON();
      expect(body.enabled).toBe(true);
      expect(body.consent_given).toBe(true);
      expect(body.wallet_address).toBe("my-wallet-address");
      await route.fulfill({ json: { status: "ok" } });
    });
    await page.click('button:has-text("Save Preferences")');
    await expect(page.getByText("Saved")).toBeVisible();
  });

  test("should interact with notifications island", async ({ page }) => {
    // Check unread count on bell
    const bell = page.locator('button[title*="Memories"]');
    // MOCK_UNREAD is 3 in TopStatusBar.tsx, we can't easily override it since it's hardcoded
    await expect(bell.locator("text=3")).toBeVisible();

    await bell.click();
    await expect(page.getByText("System Update")).toBeVisible();
    await expect(page.getByText("New Memory")).toBeVisible();

    // Filter by Memory island
    await page.click('button:has-text("Memory")');
    await expect(page.getByText("System Update")).toBeHidden();
    await expect(page.getByText("New Memory")).toBeVisible();

    // Mark as read
    await page.click('button:has-text("All")');
    // Just click backdrop to close
    await page.locator("div.fixed.inset-0.z-\\[65\\]").click();

    // Since it's mock 3, it should still show 3 or whatever TopStatusBar hardcodes
  });
});
