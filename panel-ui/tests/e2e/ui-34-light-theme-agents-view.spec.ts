import { expect, test } from "@playwright/test";

test.describe("AgentsView Theme Compliance", () => {
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

    // We must intercept auth/refresh because AuthProvider calls it immediately!
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

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    // Intercept agents API to mock an empty list
    await page.route("**/agents", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
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

    // Instead of sleep, wait for something concrete on the page
    const settingsButton = page.locator('button[aria-label="Open Control Node"], button[title="Control Node"]');

    // If login is shown, log in (auto-waiting handles the timing)
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    // Now wait for settings button to be ready and click
    await expect(settingsButton.first()).toBeVisible();
    await settingsButton.first().click();

    // Wait for ConfigModal to appear
    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(modal).toBeVisible();

    // Switch to Agents tab
    const agentsTab = page.locator('button', { hasText: /^Agents$/ });
    await expect(agentsTab).toBeVisible();
    await agentsTab.click();

    // Verify Light Mode
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const heading = page.locator('h2:has-text("Agent Manager")');
    await expect(heading).toBeVisible();
    const headingColor = await heading.evaluate((el) => window.getComputedStyle(el).color);
    // Dark text on light background usually means non-white
    expect(headingColor).not.toBe("rgb(255, 255, 255)");

    const container = heading.locator('xpath=ancestor::div[contains(@class, "space-y-8")]').first();
    const bgLuminance = await container.evaluate((el) => {
      // Container itself might not have a bg, checking the modal surface (its parent)
      const modalBg = window.getComputedStyle(el.closest('[class*="bg-"]') || document.body).backgroundColor;
      // parse rgb
      const rgb = modalBg.match(/\d+/g);
      if (!rgb) return 0;
      const [r, g, b] = rgb.map(Number);
      // Relative luminance calculation
      const a = [r, g, b].map(function (v) {
        v /= 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
    });

    // Ensure it's a light background (luminance > 0.7)
    expect(bgLuminance).toBeGreaterThan(0.7);

    // Switch to Dark Mode via appearance settings
    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await expect(moonBtn).toBeVisible();
    await moonBtn.click();

    // Go back to Agents tab
    await agentsTab.click();

    // Verify Dark Mode
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    const darkHeadingColor = await heading.evaluate((el) => window.getComputedStyle(el).color);
    expect(darkHeadingColor).toBe("rgb(255, 255, 255)");

    expect(pageErrors).toEqual([]);
  });
});
