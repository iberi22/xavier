import { test, expect } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const ARTIFACT_DIR = path.join(process.cwd(), "test-results", "artifacts");

test.describe("ConfigModal Light Theme Compatibility", () => {
  test("ConfigModal adopts light theme when studio-bone is active", async ({ page }) => {
    // 1. Intercept network
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok" }),
      });
    });

    await page.route("**/auth/login", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ token: "mock-jwt-token", user: { id: "1", email: "operator@xavier.local", role: "admin" } }),
      });
    });

    await page.route("**/v1/auth/login", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ token: "mock-jwt-token", user: { id: "1", email: "operator@xavier.local", role: "admin" } }),
      });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ token: "mock-jwt-token" }),
      });
    });

    // 2. Set light theme and bypass onboarding
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "light"); // Force light theme
      window.localStorage.setItem("auth-storage", JSON.stringify({
        state: { token: "mock-jwt-token", isAuthenticated: true, user: { id: "1" } },
        version: 0
      }));
    });

    // 3. Load app
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Verify light theme is active on document
    const html = page.locator("html");
    await expect(html).toHaveAttribute("data-theme", "studio-bone");

    // 4. Open ConfigModal
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    // 5. Verify modal uses proper theme classes, not hardcoded dark backgrounds
    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(modal).toBeVisible();

    // Target the main modal container based on its dimensions and positioning as per ConfigModal.tsx
    const configModalWrapper = page.locator('div.w-\\[960px\\]');
    await expect(configModalWrapper).toBeVisible();

    // Ensure the container is using the expected light theme variable logic
    // We check the computed background color and luminance to verify

    const bgColor = await configModalWrapper.evaluate((el) => {
      return window.getComputedStyle(el).backgroundColor;
    });

    // Parse rgb/rgba and calculate luminance
    // Light theme luminance should be > 0.7. If it returns oklab, we can assume it's Light if L > 0.7
    console.log("Background color:", bgColor);
    const isLight = bgColor.includes('oklab') ? parseFloat(bgColor.match(/oklab\(([0-9.]+)/)?.[1] || "0") > 0.7 : (() => {
        const match = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        if (match) {
            const r = parseInt(match[1]);
            const g = parseInt(match[2]);
            const b = parseInt(match[3]);
            return ((0.299 * r + 0.587 * g + 0.114 * b) / 255) > 0.7;
        }
        return false;
    })();
    expect(isLight).toBeTruthy();

    // Switch to Appearance tab and verify text is dark (using text-[var(--foreground)] instead of text-white)
    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    const heading = page.locator('h1:has-text("Appearance")');
    await expect(heading).toBeVisible();

    // It's the inner page heading so it wasn't modified, but we can verify the text color of the
    // settings tabs on the left sidebar (which are in ConfigModal.tsx).
    const accountTab = page.locator('button[data-testid="settings-tab-account"]');
    await expect(accountTab).toHaveClass(/text-\[var\(--foreground\)\]/);

    // Switch back to dark theme to ensure dark mode remains intact
    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await moonBtn.click();

    await expect(html).toHaveAttribute("data-theme", "studio-dark");

    // Let it apply the theme change

    const darkBgColor = await configModalWrapper.evaluate((el) => {
      return window.getComputedStyle(el).backgroundColor;
    });

    console.log("Dark background color:", darkBgColor);
    const isDark = darkBgColor.includes('oklab') ? parseFloat(darkBgColor.match(/oklab\(([0-9.]+)/)?.[1] || "0") < 0.4 : (() => {
        const match = darkBgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        if (match) {
            const r = parseInt(match[1]);
            const g = parseInt(match[2]);
            const b = parseInt(match[3]);
            return ((0.299 * r + 0.587 * g + 0.114 * b) / 255) < 0.4;
        }
        return false;
    })();
    expect(isDark).toBeTruthy();

    // Close modal
    await modal.click();
  });
});
