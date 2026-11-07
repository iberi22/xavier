import { expect, test } from "@playwright/test";

test.describe("WorkspaceSelector Light Theme Compliance", () => {
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

    await page.route("**/v1/system/scan", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          version: "0.10.0",
          os: "linux",
          arch: "x86_64",
          providers: [],
          workspace_id: "default",
          memory_backend: "sqlite",
        }),
      });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
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

    // If login is shown, log in (auto-waiting handles the timing)
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible().catch(() => false)) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    // Verify Light Mode on HTML element
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    // Wait for the Workspace Selector to be visible
    const wsSelectorButton = page.locator('button[title^="Active Workspace:"]');
    await expect(wsSelectorButton).toBeVisible();

    // Verify text color
    const textColor = await wsSelectorButton.locator("span.font-semibold").evaluate((el) => window.getComputedStyle(el).color);
    expect(textColor).not.toBe("rgb(255, 255, 255)"); // Text should not be white
    expect(textColor).not.toMatch(/oklab\(0\.9999/); // Not white in oklab

    // Verify background luminance
    const bgLuminance = await wsSelectorButton.evaluate((el) => {
      const bg = window.getComputedStyle(el).backgroundColor;
      // Convert OKLAB or RGB to approximate luminance for testing
      // Since background in light mode is expected to be light, just ensure it's not a dark oklab/rgb
      if (bg.includes('oklch') || bg.includes('oklab')) {
        const matches = bg.match(/[\d.]+/g);
        if (matches) {
          const l = Number(matches[0]);
          return l; // in oklab/oklch, L is luminance 0..1
        }
      }

      const rgb = bg.match(/\d+/g);
      if (!rgb) return 0;
      const [r, g, b] = rgb.map(Number);
      const a = [r, g, b].map(function (v) {
        v /= 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
    });

    expect(bgLuminance).toBeGreaterThan(0.7); // Light background

    // Open Config Modal to switch theme
    const settingsButton = page.locator('button[aria-label="Open Control Node"], button[title="Control Node"]');
    await expect(settingsButton.first()).toBeVisible();
    await settingsButton.first().click();

    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(modal).toBeVisible();

    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await expect(moonBtn).toBeVisible();
    await moonBtn.click();

    // Close modal
    await modal.click();

    // Verify Dark Mode on HTML element
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    // Re-verify the workspace selector
    await expect(wsSelectorButton).toBeVisible();

    const darkTextColor = await wsSelectorButton.locator("span.font-semibold").evaluate((el) => window.getComputedStyle(el).color);
    // Dark mode text should be white or near-white
    expect(darkTextColor).toMatch(/rgb\(255, 255, 255\)|oklab\(0\.9999/);

    // Validate the dropdown menu works in dark mode as well
    await wsSelectorButton.click();
    const popupLabel = page.locator('span:has-text("Workspaces / Vaults")').first();
    await expect(popupLabel).toBeVisible();

    const popupBgColor = await page.locator('.absolute.top-full').first().evaluate((el) => window.getComputedStyle(el).backgroundColor);

    if (popupBgColor.includes('oklch') || popupBgColor.includes('oklab')) {
        const matches = popupBgColor.match(/[\d.]+/g);
        if (matches) {
          const l = Number(matches[0]);
          expect(l).toBeLessThan(0.3);
        }
    } else {
        const popupBgRgb = popupBgColor.match(/\d+/g);
        if (popupBgRgb) {
          const [r, g, b] = popupBgRgb.map(Number);
          // Expected #0a0a0a rgb(10, 10, 10)
          expect(r).toBeLessThan(30);
          expect(g).toBeLessThan(30);
          expect(b).toBeLessThan(30);
        }
    }

    // Assert no errors occurred during render
    expect(pageErrors).toEqual([]);
  });
});
