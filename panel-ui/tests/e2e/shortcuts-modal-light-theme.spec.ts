import { expect, test } from "@playwright/test";

test.describe("Shortcuts Modal Light Theme", () => {
  test("shortcuts modal reflects light theme correctly", async ({ page }) => {
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

    // AuthProvider calls authClient.refresh() on every mount (panel-ui/src/auth/AuthProvider.tsx)
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock.jwt.access-token-maloca",
          refresh_token: "mock-refresh-token-maloca",
        }),
      });
    });

    // Bypass onboarding and set authenticated session
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

    // 1. Navigate to the app root
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Auto-wait mechanism via expect for the login form if needed
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible({timeout: 1000}).catch(()=>false)) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    // 3. Open Config Modal to inspect Appearance tab and shortcuts
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    // Wait for ConfigModal to appear
    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(modal).toBeVisible();

    // Switch to Light Mode (Sun icon)
    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await expect(appearanceSidebarBtn).toBeVisible();
    await appearanceSidebarBtn.click();

    const sunBtn = page.locator('button[aria-label="Light theme"]');
    await expect(sunBtn).toBeVisible();
    await sunBtn.click();

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    // 4. Open Shortcuts modal
    const shortcutsBtn = page.locator('button', { hasText: "Shortcuts" }).first();
    await expect(shortcutsBtn).toBeVisible();
    await shortcutsBtn.click();

    const modalContent = page.locator('.fixed.inset-0 .relative.w-full.max-w-lg');
    await expect(modalContent).toBeVisible();

    // Assert light theme specific classes exist, dark variants won't be active
    await expect(modalContent).toHaveClass(/bg-surface-card/);

    // Take screenshot in Bone White for the Shortcuts Modal
    const screenshotPath = test.info().outputPath("shortcuts_modal_light_e2e.png");
    await page.screenshot({ path: screenshotPath });
  });
});
