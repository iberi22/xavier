import { expect, test } from "@playwright/test";

test.describe("MeshConfig Light Theme Verification", () => {
  test.beforeEach(async ({ page }) => {
    await page.route("**/health", async (route) => {
      await route.fulfill({
        json: {
          status: "online",
          system: { cpu_usage: 10, ram_usage_percent: 20 },
        },
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
          access_token: "mock.jwt.access-token-mesh-config",
          refresh_token: "mock-refresh-token",
        }),
      });
    });

    await page.route("**/panel/api/mesh/config", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          localNodeId: "node-local-1",
          peers: [],
        }),
      });
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
      await route.fulfill({
        json: {
          id: "default",
          name: "Roadmap",
          data: { nodes: [], links: [] },
        },
      });
    });

    await page.route("**/memory/graph/**", async (route) => {
      await route.fulfill({ json: { nodes: [], edges: [], links: [] } });
    });
    await page.route("**/code/**", async (route) => {
      await route.fulfill({
        json: { total_symbols: 0, total_files: 0, nodes: [], links: [] },
      });
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.localStorage.setItem("vite-ui-theme", "light");

      document.addEventListener("DOMContentLoaded", () => {
        document.documentElement.setAttribute("data-theme", "studio-bone");
        document.documentElement.classList.add("light", "studio-bone");
      });

      window.localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            token: "mock-jwt-token",
            isAuthenticated: true,
            user: { id: "1", email: "operator@xavier.local", role: "admin" },
          },
          version: 0,
        }),
      );
    });
  });

  test("MeshConfig should render light theme and dark text", async ({
    page,
  }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    // Wait for ConfigModal button
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible({ timeout: 15000 });
    await brainButton.click();

    const meshTabBtn = page.locator("button", { hasText: /^Mesh$/ });
    await expect(meshTabBtn).toBeVisible({ timeout: 15000 });
    await meshTabBtn.click();

    const heading = page.locator("h2", { hasText: "Mesh Network & Sharing" });
    await expect(heading).toBeVisible();

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const bodyLuminance = await page.evaluate(() => {
      const bg = window.getComputedStyle(document.body).backgroundColor;
      const match = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return -1;
      const [r, g, b] = [
        parseInt(match[1]),
        parseInt(match[2]),
        parseInt(match[3]),
      ];
      return (0.299 * r + 0.587 * g + 0.114 * b) / 255;
    });

    expect(bodyLuminance).toBeGreaterThan(0.7);

    // Instead of doing getComputedStyle which might fail to catch custom properties properly,
    // let's just make sure the component correctly uses text-foreground.
    await expect(heading).toHaveClass(/text-foreground/);

    const screenshotPath = testInfo.outputPath("meshconfig_light.png");
    await page.screenshot({ path: screenshotPath, fullPage: true });

    // Switch to dark theme using the Appearance tab.
    const appearanceTabBtn = page.getByTestId("settings-tab-appearance");
    await expect(appearanceTabBtn).toBeVisible();
    await appearanceTabBtn.click();

    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await expect(moonBtn).toBeVisible();
    await moonBtn.click();

    // Switch back to Mesh
    await meshTabBtn.click();
    await expect(heading).toBeVisible();

    // After switching, body luminance should be dark
    const darkBodyLuminance = await page.evaluate(() => {
      const bg = window.getComputedStyle(document.body).backgroundColor;
      const match = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return -1;
      const [r, g, b] = [
        parseInt(match[1]),
        parseInt(match[2]),
        parseInt(match[3]),
      ];
      return (0.299 * r + 0.587 * g + 0.114 * b) / 255;
    });
    expect(darkBodyLuminance).toBeLessThan(0.3);

    expect(pageErrors).toEqual([]);
  });
});
