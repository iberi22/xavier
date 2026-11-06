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

test.describe("FounderNodeStatusCard Light Theme E2E", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept health and auth requests
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
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

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock.jwt.access-token-maloca",
          refresh_token: "mock-refresh-token-maloca",
        }),
      });
    });

    await page.route("**/xavier/sync/check", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          status: "ok",
          lag_ms: 12,
          save_ok_rate: 0.99,
          active_agents: 5,
        }),
      });
    });

    await page.route("**/v1/mesh/public/nodes", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([
          { node_id: "node_alpha", provider: "vps", status: "active" },
          { node_id: "node_beta", provider: "supabase", status: "active" },
        ]),
      });
    });

    // Bypass onboarding and set authenticated session with light theme
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.sessionStorage.setItem("xavier.rt", "mock-rt-token");
      window.localStorage.setItem("vite-ui-theme", "light");
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

  test("renders light theme (studio-bone) with readable colors and switches to dark theme", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Click the Founder Node Telemetry HUD Card
    const founderNodeButton = page.locator('button[aria-label="Founder Node Telemetry HUD Card"]');
    await expect(founderNodeButton).toBeVisible();
    await founderNodeButton.click();

    // Verify FounderNodeStatusCard is visible
    const statusCard = page.getByTestId("founder-node-status-card");
    await expect(statusCard).toBeVisible();

    // Verify html data-theme is studio-bone
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    // Ensure the main container computed background luminance is > 0.7
    const bgLuminance = await statusCard.evaluate((node) => {
      const bgColor = window.getComputedStyle(node).backgroundColor;
      // Match rgba?\((\d+),\s*(\d+),\s*(\d+)
      const match = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return 0;
      const r = parseInt(match[1], 10) / 255;
      const g = parseInt(match[2], 10) / 255;
      const b = parseInt(match[3], 10) / 255;

      const toLinear = (c: number) => c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
      const luminance = 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
      return luminance;
    });

    expect(bgLuminance).toBeGreaterThan(0.7);

    // Verify a heading or label is dark
    const heading = statusCard.getByText("SWAL Founder Node");
    const headingLuminance = await heading.evaluate((node) => {
      const color = window.getComputedStyle(node).color;
      const match = color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return 1;
      const r = parseInt(match[1], 10) / 255;
      const g = parseInt(match[2], 10) / 255;
      const b = parseInt(match[3], 10) / 255;

      const toLinear = (c: number) => c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
      const luminance = 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
      return luminance;
    });

    // Expect heading to be dark (low luminance)
    expect(headingLuminance).toBeLessThan(0.3);

    // Take screenshot of light mode
    await page.screenshot({ path: testInfo.outputPath("founder_node_card_light.png"), fullPage: true });

    // Close card
    await statusCard.getByLabel("Close").click();
    await expect(statusCard).toBeHidden();

    // Open Config Modal to switch theme
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    // Switch to Dark Mode (Moon icon)
    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await moonBtn.click();
    await page.waitForTimeout(600);

    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    // Close config modal
    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await modal.click();
    await page.waitForTimeout(400);

    // Open the card again
    await expect(founderNodeButton).toBeVisible();
    await founderNodeButton.click();
    await expect(statusCard).toBeVisible();

    // Verify dark mode background is dark (luminance < 0.2)
    const darkBgLuminance = await statusCard.evaluate((node) => {
      const bgColor = window.getComputedStyle(node).backgroundColor;
      const match = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return 1;
      const r = parseInt(match[1], 10) / 255;
      const g = parseInt(match[2], 10) / 255;
      const b = parseInt(match[3], 10) / 255;

      const toLinear = (c: number) => c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
      const luminance = 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
      return luminance;
    });

    expect(darkBgLuminance).toBeLessThan(0.2);

    await page.screenshot({ path: testInfo.outputPath("founder_node_card_dark.png"), fullPage: true });

    expect(pageErrors).toEqual([]);
  });
});
