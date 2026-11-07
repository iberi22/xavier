import { test, expect } from "@playwright/test";

test.describe("Maloca Light Theme E2E", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept API routes
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "ok" } });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock-access",
          refresh_token: "mock-refresh",
        }),
      });
    });

    // Mock maloca APIs
    await page.route("**/api/maloca/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    // AuthProvider check uses these APIs for generic state
    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });
  });

  test("maloca follows light and dark themes", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
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

    await page.goto("/#/maloca");

    // Wait for the main component to appear
    const root = page.locator(".maloca-root");
    await expect(root).toBeVisible();

    // Verify theme data attribute on html element
    await expect(page.locator("html")).toHaveAttribute("data-theme", "studio-bone");

    // Evaluate luminance
    const bgLuminance = await root.evaluate((el) => {
      const bgColor = window.getComputedStyle(el).backgroundColor;
      // Extract RGB values
      const match = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return 1;
      const r = parseInt(match[1]) / 255;
      const g = parseInt(match[2]) / 255;
      const b = parseInt(match[3]) / 255;
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    });

    // In light theme, background should be light (high luminance)
    expect(bgLuminance).toBeGreaterThan(0.7);

    // Verify heading is dark
    const heading = page.locator("h1");
    await expect(heading).toBeVisible();
    const headingLuminance = await heading.evaluate((el) => {
      const color = window.getComputedStyle(el).color;
      const match = color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return 0;
      const r = parseInt(match[1]) / 255;
      const g = parseInt(match[2]) / 255;
      const b = parseInt(match[3]) / 255;
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    });
    // In light theme, text should be dark (low luminance)
    expect(headingLuminance).toBeLessThan(0.3);

    await page.screenshot({ path: testInfo.outputPath("light-theme.png"), fullPage: true });

    expect(pageErrors).toEqual([]);
  });

  test("maloca follows dark theme", async ({ page }, testInfo) => {
      const pageErrors: string[] = [];
      page.on("pageerror", (err) => pageErrors.push(String(err)));

      await page.addInitScript(() => {
        window.localStorage.setItem("xavier_onboarding_completed", "true");
        window.localStorage.setItem("xavier_security_onboarding_completed", "true");
        window.localStorage.setItem("vite-ui-theme", "studio-dark"); // Dark theme
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

      await page.goto("/#/maloca");

      const root = page.locator(".maloca-root");
      await expect(root).toBeVisible();

      await expect(page.locator("html")).toHaveAttribute("data-theme", "studio-dark");

      const bgLuminanceDark = await root.evaluate((el) => {
        const bgColor = window.getComputedStyle(el).backgroundColor;
        const match = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        if (!match) return 0;
        const r = parseInt(match[1]) / 255;
        const g = parseInt(match[2]) / 255;
        const b = parseInt(match[3]) / 255;
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
      });

      expect(bgLuminanceDark).toBeLessThan(0.3);
      await page.screenshot({ path: testInfo.outputPath("dark-theme.png"), fullPage: true });

      expect(pageErrors).toEqual([]);
  });
});
