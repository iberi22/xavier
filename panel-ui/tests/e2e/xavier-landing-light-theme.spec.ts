import { test, expect } from "@playwright/test";

test.describe("XavierLandingView Theme Consistency", () => {
  test("renders light mode appropriately when studio-bone theme is active", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({
          error: "unauthorized"
        }),
      });
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "light");
      window.localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            token: null,
            isAuthenticated: false,
            user: null,
          },
          version: 0,
        })
      );
    });

    await page.goto("/#/landing");

    // Auto-waiting for root component to mount
    const rootEl = page.locator("html");
    await expect(rootEl).toHaveAttribute("data-theme", "studio-bone");

    const xavierLandingHeading = page.getByRole("heading", { name: /La memoria cognitiva/ });
    await expect(xavierLandingHeading).toBeVisible();

    // Assert on computed luminance
    const container = page.locator(".min-h-screen").first();

    const bgColor = await container.evaluate((el) => window.getComputedStyle(el).backgroundColor);

    // Extract RGB to calculate luminance
    const rgbMatch = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
    let luminance = 0;
    if (rgbMatch) {
      const r = parseInt(rgbMatch[1]) / 255;
      const g = parseInt(rgbMatch[2]) / 255;
      const b = parseInt(rgbMatch[3]) / 255;

      const toLinear = (c: number) => c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);

      luminance = 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
    }

    // Make sure we have a light background
    expect(luminance).toBeGreaterThan(0.7);

    // Verify heading is dark
    const headingColor = await xavierLandingHeading.evaluate((el) => window.getComputedStyle(el).color);
    const headingRgbMatch = headingColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
    let headingLuminance = 1;
    if (headingRgbMatch) {
      const r = parseInt(headingRgbMatch[1]) / 255;
      const g = parseInt(headingRgbMatch[2]) / 255;
      const b = parseInt(headingRgbMatch[3]) / 255;

      const toLinear = (c: number) => c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);

      headingLuminance = 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
    }
    expect(headingLuminance).toBeLessThan(0.3);

    // Take a screenshot
    await page.screenshot({ path: testInfo.outputPath("landing_light.png"), fullPage: true });

    expect(pageErrors).toEqual([]);
  });

  test("renders dark mode appropriately when dark theme is active", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({
          error: "unauthorized"
        }),
      });
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "dark");
      window.localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            token: null,
            isAuthenticated: false,
            user: null,
          },
          version: 0,
        })
      );
    });

    await page.goto("/#/landing");

    const rootEl = page.locator("html");
    await expect(rootEl).not.toHaveAttribute("data-theme", "studio-bone");

    const xavierLandingHeading = page.getByRole("heading", { name: /La memoria cognitiva/ });
    await expect(xavierLandingHeading).toBeVisible();

    // Assert on computed luminance
    const container = page.locator(".min-h-screen").first();
    const bgColor = await container.evaluate((el) => window.getComputedStyle(el).backgroundColor);

    // Extract RGB to calculate luminance
    const rgbMatch = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
    let luminance = 1; // Default to 1 to fail if we don't parse it
    if (rgbMatch) {
      const r = parseInt(rgbMatch[1]) / 255;
      const g = parseInt(rgbMatch[2]) / 255;
      const b = parseInt(rgbMatch[3]) / 255;

      const toLinear = (c: number) => c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);

      luminance = 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
    }

    // Make sure we have a dark background
    expect(luminance).toBeLessThan(0.3);

    await page.screenshot({ path: testInfo.outputPath("landing_dark.png"), fullPage: true });

    expect(pageErrors).toEqual([]);
  });
});
