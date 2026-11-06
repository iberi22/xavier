import { test, expect } from "@playwright/test";
test.describe("Wallet Light Theme", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept API routes
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock.jwt.access-token",
          refresh_token: "mock-refresh-token",
        }),
      });
    });

    // Seed light theme and authenticated session
    await page.addInitScript(() => {
      window.localStorage.setItem("vite-ui-theme", "light");
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            token: "mock-jwt-token",
            isAuthenticated: true,
            user: { id: "node_test", email: "test@xavier.local", role: "admin", karma: 10 },
          },
          version: 0,
        })
      );
    });
  });

  test("wallet UI matches light theme colors and dark mode works", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    await page.goto("/#/wallet");

    // Wait for the modal container
    const modal = page.getByRole("dialog").or(page.locator(".fixed.inset-0.z-50").first());
    await expect(modal).toBeVisible();

    // The root HTML should have data-theme="studio-bone"
    await expect(page.locator("html")).toHaveAttribute("data-theme", "studio-bone");

    // Grab the main container of the wallet view
    // It's the inner div with max-w-4xl
    const mainContainer = page.locator(".max-w-4xl");
    await expect(mainContainer).toBeVisible();

    // Check main container background luminance
    const bgResult = await mainContainer.evaluate((el) => {
      const style = window.getComputedStyle(el);
      const bg = style.backgroundColor;
      // expecting rgb(255, 255, 255) for bg-white
      // Luminance formula for sRGB
      const match = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return -1;
      const [_, r, g, b] = match;
      const rF = parseInt(r, 10) / 255;
      const gF = parseInt(g, 10) / 255;
      const bF = parseInt(b, 10) / 255;
      const lR = rF <= 0.03928 ? rF / 12.92 : Math.pow((rF + 0.055) / 1.055, 2.4);
      const lG = gF <= 0.03928 ? gF / 12.92 : Math.pow((gF + 0.055) / 1.055, 2.4);
      const lB = bF <= 0.03928 ? bF / 12.92 : Math.pow((bF + 0.055) / 1.055, 2.4);
      return 0.2126 * lR + 0.7152 * lG + 0.0722 * lB;
    });

    expect(bgResult).toBeGreaterThan(0.7);

    // Check heading color
    const heading = page.locator("h2").filter({ hasText: "Billetera Soberana SWAL" });
    const colorResult = await heading.evaluate((el) => {
      return window.getComputedStyle(el).color;
    });
    // Should be dark text: rgb(15, 23, 42) -> slate-900
    // As long as it's not white rgb(255, 255, 255)
    expect(colorResult).not.toBe("rgb(255, 255, 255)");
    expect(colorResult).not.toBe("rgba(255, 255, 255, 1)");

    await page.screenshot({ path: testInfo.outputPath("wallet-light-mode.png"), fullPage: true });

    // Switch to dark mode
    await page.evaluate(() => {
      window.localStorage.setItem("vite-ui-theme", "dark");
    });
    // Let's use playwright context addInitScript to overwrite before reload
    await page.addInitScript(() => {
      window.localStorage.setItem("vite-ui-theme", "dark");
    });
    await page.reload();

    await expect(page.locator("html")).toHaveAttribute("data-theme", "studio-dark");

    const darkHeading = page.locator("h2").filter({ hasText: "Billetera Soberana SWAL" });
    const darkColorResult = await darkHeading.evaluate((el) => {
      return window.getComputedStyle(el).color;
    });
    // Should be white
    expect(darkColorResult).toBe("rgb(255, 255, 255)");

    const darkBgResult = await mainContainer.evaluate((el) => {
      const style = window.getComputedStyle(el);
      const bg = style.backgroundColor;
      // expecting rgb(10, 12, 16) for bg-[#0a0c10]
      const match = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return -1;
      const [_, r, g, b] = match;
      const rF = parseInt(r, 10) / 255;
      const gF = parseInt(g, 10) / 255;
      const bF = parseInt(b, 10) / 255;
      const lR = rF <= 0.03928 ? rF / 12.92 : Math.pow((rF + 0.055) / 1.055, 2.4);
      const lG = gF <= 0.03928 ? gF / 12.92 : Math.pow((gF + 0.055) / 1.055, 2.4);
      const lB = bF <= 0.03928 ? bF / 12.92 : Math.pow((bF + 0.055) / 1.055, 2.4);
      return 0.2126 * lR + 0.7152 * lG + 0.0722 * lB;
    });
    // should be very dark, close to 0
    expect(darkBgResult).toBeLessThan(0.1);

    await page.screenshot({ path: testInfo.outputPath("wallet-dark-mode.png"), fullPage: true });

    expect(pageErrors).toEqual([]);
  });
});
