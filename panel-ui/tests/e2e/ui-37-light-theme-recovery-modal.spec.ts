import { expect, test } from "@playwright/test";

test.describe("RecoveryModal Theme Compliance", () => {
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

    // Navigate to WalletView
    await page.goto("/#/wallet");
    await page.waitForLoadState("domcontentloaded");

    // Click on Recovery BIP-39 button to open RecoveryModal
    const recoveryBtn = page.locator("button:has-text('Recuperación BIP-39 (Share 2)')");
    await expect(recoveryBtn).toBeVisible({ timeout: 10000 });
    await recoveryBtn.click();

    // Verify modal is open
    const heading = page.locator('h2:has-text("Disaster Recovery Wizard")');
    await expect(heading).toBeVisible();

    // Verify Light Mode
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const headingColor = await heading.evaluate((el) => window.getComputedStyle(el).color);
    // Dark text on light background usually means non-white
    expect(headingColor).not.toBe("rgb(255, 255, 255)");

    const container = page.locator('div.max-w-2xl');
    const bgLuminance = await container.evaluate((el) => {
      const modalBg = window.getComputedStyle(el).backgroundColor;
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

    // Switch to Dark Mode via JavaScript injection
    await page.evaluate(() => {
      window.localStorage.setItem('vite-ui-theme', 'dark');
      window.dispatchEvent(new Event('storage'));
      document.documentElement.classList.remove('light', 'studio-bone');
      document.documentElement.classList.add('dark');
      document.documentElement.setAttribute('data-theme', 'studio-dark');
    });

    // We must wait a bit or just explicitly expect the attribute to change
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    const darkHeadingColor = await heading.evaluate((el) => window.getComputedStyle(el).color);
    expect(darkHeadingColor).toBe("rgb(255, 255, 255)");

    expect(pageErrors).toEqual([]);
  });
});
