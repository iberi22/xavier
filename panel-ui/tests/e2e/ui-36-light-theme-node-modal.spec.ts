import { test, expect } from "@playwright/test";

test.describe("NodeConnectionModal Light Theme Compliance", () => {
  test.beforeEach(async ({ page }) => {
    // Set onboarding completed flag and authenticated session in localStorage
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_auth_token", "mock-jwt-token");
    });
    // Intercept health check and system scan
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok" }),
      });
    });

    await page.route("**/v1/system/scan", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ version: "0.1", os: "linux", arch: "x86_64", providers: [], workspace_id: "default" }),
      });
    });

    // Intercept authentication
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ access_token: "mock-jwt-token", refresh_token: "mock-rt-token" }),
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
  });

  test("adapts correctly to light and dark modes without hardcoded dark colors", async ({ page }, testInfo) => {
    // Test Light Mode
    await page.goto("/");
    await page.evaluate(() => {
      window.localStorage.setItem("vite-ui-theme", "light");
    });
    // Reload to ensure theme applies cleanly
    await page.reload();

    await expect(page.locator("html")).toHaveAttribute("data-theme", "studio-bone");

    // Open Node Connection Modal via Node Connection Settings button
    const nodeSettingsBtn = page.locator("button[aria-label='Node Connection Settings']");
    await expect(nodeSettingsBtn).toBeVisible();
    await nodeSettingsBtn.click();

    const heading = page.getByText("Xavier Node Connectivity");
    await expect(heading).toBeVisible();

    // The component wrapper
    const modalWrapper = heading.locator("xpath=ancestor::div[contains(@class, 'bg-[var(--surface-card)]')][1]");

    // Check modal computed background color in light mode
    // studio-bone's --surface-card is #ffffff
    await expect(modalWrapper).toHaveCSS("background-color", "rgb(255, 255, 255)");

    // Check heading color (should be dark in light mode, --foreground is #1c1c1f)
    await expect(heading).toHaveCSS("color", "rgb(28, 28, 31)");

    // Save screenshot of light mode
    await page.screenshot({ path: testInfo.outputPath("node-connection-modal-light.png") });

    // Test Dark Mode
    await page.evaluate(() => {
      window.localStorage.setItem("vite-ui-theme", "dark");
    });
    await page.reload();

    await expect(page.locator("html")).not.toHaveAttribute("data-theme", "studio-bone");

    await expect(nodeSettingsBtn).toBeVisible();
    await nodeSettingsBtn.click();

    await expect(heading).toBeVisible();

    // Check modal computed background color in dark mode
    // dark mode's --surface-card is #141414
    await expect(modalWrapper).toHaveCSS("background-color", "rgb(33, 35, 40)");

    // Check heading color (should be light in dark mode, --foreground is #e5e7eb)
    await expect(heading).toHaveCSS("color", "rgb(229, 231, 235)");

    // Save screenshot of dark mode
    await page.screenshot({ path: testInfo.outputPath("node-connection-modal-dark.png") });
  });
});
