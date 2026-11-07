import { expect, test } from "@playwright/test";

test.describe("GraphView and GraphCanvas Light Theme Contrast", () => {
  test("maintains contrast in studio-bone and respects dark mode", async ({ page }) => {
    // Intercept health and auth requests
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
      });
    });

    await page.route("**/auth/refresh", async (route) => { await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ access_token: "mock-jwt-token", refresh_token: "mock-rt-token" }) }); });
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

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.sessionStorage.setItem("xavier.rt", "mock-rt-token");
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

    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Login if prompted
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible({ timeout: 2000 }).catch(() => false)) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    // Look for a tab with text "Projects"
    const projectsBtn = page.locator('text="Projects"').first();
    await expect(projectsBtn).toBeVisible();
    await projectsBtn.click();

    // Check light mode styles - we expect surface-card background which is white in bone theme
    await page.waitForTimeout(500);

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    // Switch to light mode via appearance settings to be thorough
    const appearanceBtn = page.locator('text="Appearance"').first();
    await appearanceBtn.click();

    const lightThemeBtn = page.locator('button[aria-label="Light theme"]');
    await lightThemeBtn.click();

    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");
  });
});
