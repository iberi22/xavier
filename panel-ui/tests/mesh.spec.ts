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

    // Mock Mesh Identity
    await page.route("**/v1/mesh/identity", async (route) => {
      await route.fulfill({
        json: {
          node_id: "xv1-test-node-123",
          public_key_hex: "0123456789abcdef",
        },
      });
    });

    // Mock Peers
    await page.route("**/v1/mesh/peers", async (route) => {
      if (route.request().method() === "GET") {
        await route.fulfill({
          json: [
            {
              node_id: "peer-1",
              alias: "Cloud Relay",
              endpoint_url: "https://relay.xavier.local",
              public_key_hex: "abc...",
              added_at: Date.now() / 1000,
              last_seen_at: Date.now() / 1000,
              sync_enabled: true,
              is_cloud: true,
            },
          ],
        });
      } else {
        await route.fulfill({ json: { status: "ok" } });
      }
    });

    // Mock Pairing
    await page.route("**/v1/mesh/pairing/generate", async (route) => {
      await route.fulfill({
        json: { code: "MOCK-CODE-123", secret: "mock-secret-456" },
      });
    });

    await page.route("**/v1/mesh/pairing/join", async (route) => {
      await route.fulfill({ json: { status: "ok", node_id: "peer-2" } });
    });

    // Mock Data Commons
    await page.route("**/v1/mesh/data_commons/opt_in", async (route) => {
      if (route.request().method() === "GET") {
        await route.fulfill({
          json: {
            status: "ok",
            data: {
              enabled: true,
              consent_given: true,
              wallet_address: "HN7c...",
            },
          },
        });
      } else {
        await route.fulfill({ json: { status: "ok" } });
      }
    });

    // Mock panel core data
    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    // Bypass Auth
    await page.addInitScript(() => {
      (window as any).__TAURI_INTERNALS__ = {
        invoke: async (cmd: string) => {
          if (cmd === "get_xavier_token") return "mock-token";
          if (cmd === "get_current_config_state") return { has_openai: true };
          return null;
        },
      };
      localStorage.setItem("xavier_onboarding_completed", "true");
    });

    await page.goto("/");
  });

  test("should manage mesh peers and pairing", async ({ page }) => {
    // Open Config
    await page.getByTitle("Open Control Node").click();

    // Go to Network tab
    await page.getByRole("button", { name: "Network" }).click();

    // Verify Identity
    await expect(page.getByText("xv1-test-node-123")).toBeVisible();

    // Verify Peer list
    await expect(page.getByText("Cloud Relay")).toBeVisible();

    // Generate pairing code
    await page.getByRole("button", { name: "Generate Pairing Code" }).click();
    await expect(page.getByText("MOCK-CODE-123")).toBeVisible();
    await expect(page.getByText("mock-secret-456")).toBeVisible();

    // Join Mesh
    await page.getByPlaceholder("Paste pairing code here...").fill("REMOTE-CODE");
    await page.getByRole("button", { name: "Join" }).click();
    // Re-loads data on success
  });

  test("should show data commons dashboard", async ({ page }) => {
    // Open Config
    await page.getByTitle("Open Control Node").click();

    // Go to Server & Network sub-tab in Configuration
    await page.getByRole("button", { name: "Configuration" }).click();
    await page.getByRole("button", { name: "Server & Network" }).click();

    // Verify Stats
    await expect(page.getByText("Estimated $XAV")).toBeVisible();
    await expect(page.getByText("452.80")).toBeVisible();

    // Verify Telemetry toggle
    await expect(page.getByText("Enable Data Telemetry")).toBeVisible();
  });
});
