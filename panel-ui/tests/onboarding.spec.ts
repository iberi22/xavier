import { expect, test } from "@playwright/test";

test.describe("Onboarding Flow", () => {
  test.beforeEach(async ({ page }) => {
    // Mock Tauri internals and invoke
    await page.addInitScript(() => {
      (window as any).__TAURI_INTERNALS__ = {
        invoke: async (cmd: string, args: any) => {
          console.log("Tauri Invoke:", cmd, args);
          if (cmd === "scan_system") {
            return {
              total_ram_gb: 16.0,
              cpu_cores: 8,
              has_gpu: true,
              openclaw_running: true,
              hermes_running: false,
            };
          }
          if (cmd === "save_initial_config") {
            return null;
          }
          if (cmd === "get_xavier_token") {
            return "mock-token";
          }
          if (cmd === "get_current_config_state") {
            return { has_openai: false, has_gemini: false };
          }
          if (cmd === "get_realtime_metrics") {
            return { cpu_percent: 10, ram_used_gb: 4, ram_total_gb: 16 };
          }
          return null;
        },
      };
    });

    // Mock health endpoint
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "online", version: "1.0.0" }),
      });
    });

    // Mock threads and other initial data to prevent crashes after onboarding
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

    // Clear onboarding state before each test
    await page.evaluate(() => {
      localStorage.removeItem("xavier_onboarding_completed");
      window.location.reload();
    });

    // Wait for reload and re-check if we are on onboarding
    await page.waitForURL("/");
  });

  test("should complete the full onboarding flow", async ({ page }) => {
    // Step 0: Welcome
    await expect(page.getByText("INITIALIZING XAVIER_")).toBeVisible();
    await page.getByRole("button", { name: "BEGIN_SCAN" }).click();

    // Step 1: System Scan
    await expect(page.getByText("SYSTEM_DIAGNOSTICS")).toBeVisible();
    await expect(
      page.getByText("> Initiating deep system scan..."),
    ).toBeVisible();

    // The component has 1000ms, 800ms, 800ms delays, then 2000ms before transition
    // Wait for the transition to Hardware Step
    await expect(page.getByText("NEURAL_EXECUTION_PLAN")).toBeVisible({
      timeout: 10000,
    });

    // Step 2: Hardware
    await expect(page.getByText("GPU Accleration")).toBeVisible();
    await expect(page.getByText("GPU Detected: YES")).toBeHidden(); // Log from previous step

    // Verify default selection (GPU was true in mock)
    await expect(page.getByText("gpu-fast-model")).toBeVisible();

    // Toggle to CPU
    await page.getByText("CPU Fallback").click();
    await expect(page.getByText("cpu-fast-model")).toBeVisible();

    await page.getByRole("button", { name: "CONFIRM_ALLOCATION" }).click();

    // Step 3: Integrations
    await expect(page.getByText("EXTERNAL_UPLINK")).toBeVisible();
    await page
      .getByPlaceholder("123456789:ABCdefGHIjklMNOpqrsTUVwxyz")
      .fill("test-bot-token");

    await page.getByRole("button", { name: "INITIALIZE_SYSTEM" }).click();

    // Final state: Should show Chat screen (since we mocked get_xavier_token)
    await expect(page.getByPlaceholder("Initialize command sequence...")).toBeVisible();

    const completed = await page.evaluate(() =>
      localStorage.getItem("xavier_onboarding_completed"),
    );
    expect(completed).toBe("true");
  });
});
