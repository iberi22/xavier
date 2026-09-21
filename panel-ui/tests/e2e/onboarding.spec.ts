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

test.describe("Onboarding Flow (pending wave-theme)", () => {
  test.beforeEach(async ({ page }) => {
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

    await page.addInitScript(() => {
      localStorage.removeItem("xavier_onboarding_completed");
      localStorage.removeItem("xavier_token");
      localStorage.removeItem("xavier_session_active");
    });

    await page.goto("/");
  });

  test("Cloud / Browser Flow (xavier.swal.network): Zero-Knowledge onboarding without 401 console errors", async ({
    page,
  }) => {
    const consoleErrors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") {
        consoleErrors.push(msg.text());
      }
    });

    await page.goto("/");

    // Step 0: Welcome Step in Studio Dark
    const html = page.locator("html");
    await expect(html).toHaveAttribute("data-theme", "studio-dark");
    await expect(page.getByText("INITIALIZING XAVIER_")).toBeVisible();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "onboarding_step_0_welcome.png"), fullPage: true });
    await page.getByRole("button", { name: /BEGIN_SCAN/i }).click();

    // Step 1: System Scan
    await expect(page.getByText("SYSTEM_DIAGNOSTICS")).toBeVisible();
    await expect(page.getByText("> Initiating deep system scan...")).toBeVisible();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "tauri_step_1_system_scan.png"), fullPage: true });
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "onboarding_step_1_system_scan.png"), fullPage: true });

    // Step 2: Hardware Step
    await expect(page.getByText("NEURAL_EXECUTION_PLAN")).toBeVisible({
      timeout: 10000,
    });
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "onboarding_step_2_hardware.png"), fullPage: true });
    await page.getByRole("button", { name: "CONFIRM_ALLOCATION" }).click();

    // Step 3: Integrations Step
    await expect(page.getByText("EXTERNAL_UPLINK")).toBeVisible();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "onboarding_step_3_integrations.png"), fullPage: true });
    await page.getByRole("button", { name: "INITIALIZE_SYSTEM" }).click();

    // Step 4: Auth Step
    await expect(page.getByText("Create Account")).toBeVisible();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "onboarding_step_4_auth.png"), fullPage: true });
    await page.getByRole("button", { name: "Skip" }).click();

    // Verification: Onboarding completed flag set
    const completed = await page.evaluate(() =>
      localStorage.getItem("xavier_onboarding_completed"),
    );
    expect(completed).toBe("true");

    // Critical assertion: No 401 unauthorized errors occurred in console
    const unauthErrors = consoleErrors.filter((e) => e.includes("401"));
    expect(unauthErrors).toHaveLength(0);
  });

  test("Desktop Local Flow (Tauri): Deep system diagnostic scan and allocation", async ({
    page,
  }) => {
    await page.addInitScript(() => {
      (window as any).__TAURI_INTERNALS__ = {
        invoke: async (cmd: string, args: any) => {
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
          return null;
        },
      };
      localStorage.removeItem("xavier_onboarding_completed");
    });

    await page.goto("/");

    // Step 0: Welcome Step
    await expect(page.getByText("INITIALIZING XAVIER_")).toBeVisible();
    await page.getByRole("button", { name: /BEGIN_SCAN/i }).click();

    // Step 1: System Scan
    await expect(page.getByText("SYSTEM_DIAGNOSTICS")).toBeVisible();
    await expect(page.getByText("> Initiating deep system scan...")).toBeVisible();

    // Step 2: Hardware Step
    await expect(page.getByText("NEURAL_EXECUTION_PLAN")).toBeVisible({
      timeout: 10000,
    });
    // Wait for the mock to resolve, and UI to update
    await expect(page.getByText("GPU Accleration")).toBeVisible({ timeout: 10000 });

    // CPU fallback
    await page.getByText("CPU Fallback").click();
    await page.getByRole("button", { name: "CONFIRM_ALLOCATION" }).click();

    // Step 3: Integrations Step
    await expect(page.getByText("EXTERNAL_UPLINK")).toBeVisible();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "tauri_step_3_integrations.png"), fullPage: true });
    await page.getByRole("button", { name: "INITIALIZE_SYSTEM" }).click();

    // Step 4: Auth Step
    await expect(page.getByText("Create Account")).toBeVisible();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "tauri_step_4_auth.png"), fullPage: true });
    await page.getByRole("button", { name: "Skip" }).click();

    await expect
      .poll(
        async () =>
          await page.evaluate(() =>
            localStorage.getItem("xavier_onboarding_completed"),
          ),
      )
      .toBe("true");
  });
});
