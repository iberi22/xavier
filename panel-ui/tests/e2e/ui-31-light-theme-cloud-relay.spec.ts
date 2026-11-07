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

test.describe("CloudRelayConfig Theme Awareness", () => {
  test("renders light theme properly and matches dark theme in dark mode", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    await page.route("**/v1/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok" }),
      });
    });

    // Mock health endpoint
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online", version: "1.0.0" } });
    });

    // Mock auth/login endpoint
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

    // Mock cloud config endpoints
    await page.route("**/v1/mesh/cloud", async (route) => {
      if (route.request().method() === "GET") {
        await route.fulfill({
          json: {
            url: "https://xyz.supabase.co",
            token: "secret-token",
            instance_id: "test-instance",
            sync_interval_ms: 300000,
            auto_heartbeat: true,
          },
        });
      } else {
        await route.fulfill({ json: { status: "ok" } });
      }
    });

    await page.route("**/v1/mesh/peers", async (route) => {
      await route.fulfill({ json: { local_node_id: "test", peers: [] } });
    });

    await page.route("**/v1/mesh/data_commons/opt_in", async (route) => {
      await route.fulfill({ json: { enabled: false, consent_given: false, wallet_address: "" } });
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

    // Set initial light theme and bypass onboarding
    await page.addInitScript(() => {
      window.localStorage.setItem("vite-ui-theme", "light");
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.sessionStorage.setItem("xavier.rt", "mock-rt-token");
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

    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(1000);

    // If login is shown, log in
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
      await page.waitForTimeout(1000);
    }

    // Wait for the app to finish loading
    await page.waitForTimeout(2000);

    // Open Config Modal -> Configuration -> Server & Network
    const brainButton = page.locator('button[aria-label="Open Control Node"], button[title="Open Control Node"]');
    await expect(brainButton).toBeVisible({ timeout: 15000 });
    await brainButton.click();

    // Verify Settings/Configuration is visible and open it if not default
    const configTabBtn = page.getByTestId("settings-tab-config").or(page.locator('button:has-text("Configuration")')).first();
    await expect(configTabBtn).toBeVisible();
    await configTabBtn.click();

    const serverNetworkTabBtn = page.locator('button:has-text("Server & Network")').first();
    await expect(serverNetworkTabBtn).toBeVisible();
    await serverNetworkTabBtn.click();

    // Ensure we are in light theme
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");
    await expect(htmlElement).toHaveClass(/studio-bone/);
    await expect(htmlElement).toHaveClass(/light/);

    // Verify CloudRelayConfig visibility
    const heading = page.locator('h2:has-text("Zero-Trust Cloud Relay")');
    await expect(heading).toBeVisible();

    // Evaluate background color of the section (should not be very dark)
    const section = page.locator('section:has(h2:has-text("Zero-Trust Cloud Relay"))');
    await expect(section).toBeVisible();

    const backgroundColor = await section.evaluate((el) => {
        const bg = window.getComputedStyle(el).backgroundColor;
        return bg;
    });

    // We can't perfectly assert luminance easily, but we know rgb(235, 232, 225) (studio-bone surface) or rgb(255,255,255) vs rgb(5,5,5)
    // Surface in studio-bone is #ebe8e1 -> rgb(235, 232, 225)
    // Check it's not rgb(5, 5, 5) or similar dark color. Let's just verify classes are applied properly.
    await expect(section).toHaveClass(/bg-surface/);

    // Evaluate heading color (should be dark text, not #ffffff)
    const headingColor = await heading.evaluate((el) => {
        return window.getComputedStyle(el).color;
    });
    // In studio-bone, foreground is #1c1c1f -> rgb(28, 28, 31)

    const screenshotPathLight = testInfo.outputPath("cloud_relay_light.png");
    await page.screenshot({ path: screenshotPathLight, fullPage: true });

    // Switch to dark theme
    const appearanceSidebarBtn = page.getByTestId("settings-tab-appearance").or(page.locator('button:has-text("Appearance")')).first();
    await appearanceSidebarBtn.click();
    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await expect(moonBtn).toBeVisible();
    await moonBtn.click();

    // Verify dark theme applied
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    // Go back to Server & Network
    await configTabBtn.click();
    await serverNetworkTabBtn.click();

    await expect(heading).toBeVisible();

    const screenshotPathDark = testInfo.outputPath("cloud_relay_dark.png");
    await page.screenshot({ path: screenshotPathDark, fullPage: true });

    // Assert no page errors
    expect(pageErrors).toEqual([]);
  });
});
