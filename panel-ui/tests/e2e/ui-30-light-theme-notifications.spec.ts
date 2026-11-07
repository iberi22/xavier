import { expect, test } from "@playwright/test";

test.describe("Notifications Dropdown Theme Compliance", () => {
  test("adapts to light and dark themes correctly", async ({ page }) => {
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
          access_token: "mock-jwt-token",
          refresh_token: "mock-rt-token",
        }),
      });
    });

    await page.route("**/v1/auth/login", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          token: "mock-jwt-token",
          user: { id: "1", email: "operator@xavier.local", role: "admin" },
        }),
      });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    // Intercept notifications API
    await page.route("**/notifications", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([
          {
            id: "e2e-1",
            islandId: "system",
            title: "E2E Notification",
            body: "Notification body from mock.",
            timestamp: new Date().toISOString(),
            read: false,
            severity: "info",
          },
        ]),
      });
    });

    // Seed light theme and authenticated state
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
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

    // Capture errors
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    // Navigate to app and wait for load
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // If login is shown, log in
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible({ timeout: 2000 }).catch(() => false)) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    const bellButton = page.locator('button[aria-label="Notifications"]');
    await expect(bellButton).toBeVisible();
    await bellButton.click();

    // Verify Light Mode
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const heading = page.locator('span:has-text("Notifications")').first();
    await expect(heading).toBeVisible();
    const headingColor = await heading.evaluate((el) => window.getComputedStyle(el).color);

    // In light mode, text should be dark
    expect(headingColor).not.toBe("rgb(255, 255, 255)");

    const container = heading.locator('xpath=ancestor::div[contains(@class, "flex-col")]').first();
    const bgLuminance = await container.evaluate((el) => {
      const modalBg = window.getComputedStyle(el).backgroundColor;
      const rgb = modalBg.match(/\d+/g);
      if (!rgb) return 0;
      const [r, g, b] = rgb.map(Number);
      const a = [r, g, b].map(function (v) {
        v /= 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
    });

    // Ensure it's a light background (luminance > 0.7)
    expect(bgLuminance).toBeGreaterThan(0.7);

    // Close notifications
    await page.locator('button[aria-label="Close notifications"]').click();

    // Switch to Dark Mode via appearance settings
    const settingsButton = page.locator('button[aria-label="Open Control Node"], button[title="Control Node"]');
    await expect(settingsButton.first()).toBeVisible();
    await settingsButton.first().click();

    // Wait for ConfigModal to appear
    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"], button[aria-label="Close configuration window"], .fixed.inset-0.z-40');
    await expect(modal).toBeVisible();

    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await expect(moonBtn).toBeVisible();
    await moonBtn.click();

    // Close modal
    await page.keyboard.press('Escape');

    // Open notifications again
    await bellButton.click();
    await expect(heading).toBeVisible();

    // Verify Dark Mode
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    const darkHeadingColor = await heading.evaluate((el) => window.getComputedStyle(el).color);
    // Allowing for various formats of white or near-white like rgba(255, 255, 255, 0.8)
    expect(darkHeadingColor).toMatch(/255, 255, 255|oklab\(0\.99999/);

    expect(pageErrors).toEqual([]);
  });
});
