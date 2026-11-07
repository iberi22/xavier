import { test, expect } from "@playwright/test";
test.describe("Cloud Relay Config - Theme Support", () => {
  test.beforeEach(async ({ page }, testInfo) => {
    // Intercept API routes to avoid real network calls
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    await page.route("**/panel/api/cloud-node", async (route) => {
      if (route.request().method() === "GET") {
        await route.fulfill({
          json: {
            status: "ok",
            data: {
              url: "https://mock.supabase.co",
              token: "mock-token",
              instance_id: "mock-instance",
              sync_interval_ms: 300000,
              auto_heartbeat: true
            }
          }
        });
      } else if (route.request().method() === "PUT" || route.request().method() === "POST") {
        await route.fulfill({ json: { status: "ok" } });
      }
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
          access_token: "mock-access-token",
          refresh_token: "mock-refresh-token",
        }),
      });
    });

    // Bypass onboarding and set authenticated session
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
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

  function getLuminance(r: number, g: number, b: number) {
    const a = [r, g, b].map((v) => {
      v /= 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    });
    return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
  }

  function parseRGB(rgbString: string) {
    const match = rgbString.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
    if (!match) return null;
    return {
      r: parseInt(match[1], 10),
      g: parseInt(match[2], 10),
      b: parseInt(match[3], 10),
    };
  }

  test("Cloud Relay config supports light theme (studio-bone)", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    // Seed light mode
    await page.addInitScript(() => {
      window.localStorage.setItem("vite-ui-theme", "studio-bone");
    });

    await page.goto("/");
    await page.locator('button[title="Open Control Node"]').click();
    await page.getByRole("button", { name: "Configuration" }).click();
    await page.getByRole("button", { name: "Network" }).click();

    // We can also try clicking on any element that renders the ConfigModal or navigate to a tab
    // Let's look for "Zero-Trust Cloud Relay" heading which is inside CloudRelayConfig
    const heading = page.getByRole('heading', { name: 'Zero-Trust Cloud Relay' });
    await expect(heading).toBeVisible({ timeout: 10000 });

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const section = page.locator("section").filter({ hasText: "Zero-Trust Cloud Relay" });
    await expect(section).toBeVisible();

    // Check background luminance
    const bgStr = await section.evaluate((el) => window.getComputedStyle(el).backgroundColor);
    const rgb = parseRGB(bgStr);
    expect(rgb).not.toBeNull();
    const luminance = getLuminance(rgb!.r, rgb!.g, rgb!.b);

    // In studio-bone, background luminance should be > 0.7 (light mode)
    expect(luminance).toBeGreaterThan(0.7);

    // Check heading color (should be dark in light mode)
    const headingColor = await heading.evaluate((el) => window.getComputedStyle(el).color);
    const headingRgb = parseRGB(headingColor);
    expect(headingRgb).not.toBeNull();
    const headingLuminance = getLuminance(headingRgb!.r, headingRgb!.g, headingRgb!.b);

    // Text should be dark (luminance < 0.3)
    expect(headingLuminance).toBeLessThan(0.3);

    await page.screenshot({ path: testInfo.outputPath("cloud-relay-light-mode.png") });

    expect(pageErrors).toEqual([]);
  });

  test("Cloud Relay config remains dark in dark mode", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    // Seed dark mode
    await page.addInitScript(() => {
      window.localStorage.setItem("vite-ui-theme", "studio-dark");
    });

    await page.goto("/");
    await page.locator('button[title="Open Control Node"]').click();
    await page.getByRole("button", { name: "Configuration" }).click();
    await page.getByRole("button", { name: "Network" }).click();

    const heading = page.getByRole('heading', { name: 'Zero-Trust Cloud Relay' });
    await expect(heading).toBeVisible({ timeout: 10000 });

    const section = page.locator("section").filter({ hasText: "Zero-Trust Cloud Relay" });

    // Check background luminance
    const bgStr = await section.evaluate((el) => window.getComputedStyle(el).backgroundColor);
    const rgb = parseRGB(bgStr);
    expect(rgb).not.toBeNull();
    const luminance = getLuminance(rgb!.r, rgb!.g, rgb!.b);

    // In dark mode, background luminance should be < 0.3
    expect(luminance).toBeLessThan(0.3);

    // Check heading color (should be light in dark mode)
    const headingColor = await heading.evaluate((el) => window.getComputedStyle(el).color);
    const headingRgb = parseRGB(headingColor);
    expect(headingRgb).not.toBeNull();
    const headingLuminance = getLuminance(headingRgb!.r, headingRgb!.g, headingRgb!.b);

    // Text should be light (luminance > 0.7)
    expect(headingLuminance).toBeGreaterThan(0.7);

    await page.screenshot({ path: testInfo.outputPath("cloud-relay-dark-mode.png") });

    expect(pageErrors).toEqual([]);
  });
});
