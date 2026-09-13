import { expect, test } from "@playwright/test";
import path from "node:path";

const ARTIFACT_DIR = "/home/belal/.gemini/antigravity/brain/93991307-c184-424b-87e0-d141afcaa915";

test.describe("Xavier Studio Dark & Bone White E2E Visual Verification", () => {
  test("loads Studio Dark by default, captures screenshots, and verifies theme switching", async ({
    page,
  }) => {
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

    // Bypass onboarding and set authenticated session
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
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
    await page.waitForTimeout(1000);

    // If login is shown, log in
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      await page.fill('input[type="password"]', "password123");
      await loginButton.click();
      await page.waitForTimeout(1000);
    }

    // 2. Verify Studio Dark is applied by default
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");
    await expect(htmlElement).toHaveClass(/dark/);
    await expect(htmlElement).toHaveClass(/studio-dark/);

    // Take screenshot of main shell in Studio Dark
    const studioDarkPath = path.join(ARTIFACT_DIR, "studio_dark_e2e.png");
    await page.screenshot({ path: studioDarkPath, fullPage: true });

    // 3. Open Config Modal to inspect Appearance tab
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    // Wait for ConfigModal to appear
    const modal = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(modal).toBeVisible();

    // Click on "Aspecto" tab
    const appearanceTab = page.locator('button:has-text("Aspecto")');
    await expect(appearanceTab).toBeVisible();
    await appearanceTab.click();
    await page.waitForTimeout(800);

    // Verify Appearance content
    await expect(page.locator('h2:has-text("Aspecto y Temas")')).toBeVisible();
    await expect(page.locator('span:has-text("Studio Dark")').first()).toBeVisible();
    await expect(page.locator('span:has-text("Predeterminado")').first()).toBeVisible();
    await expect(page.locator('span:has-text("Hueso Blanco")').first()).toBeVisible();
    await expect(page.locator('span:has-text("Xavier Cyberpunk")').first()).toBeVisible();

    // Take screenshot of Appearance settings modal in Studio Dark
    const appearanceModalPath = path.join(ARTIFACT_DIR, "appearance_modal_e2e.png");
    await page.screenshot({ path: appearanceModalPath, fullPage: true });

    // 4. Switch to "Hueso Blanco" (Bone White)
    const boneCard = page.locator('div[role="button"]:has-text("Hueso Blanco")').first();
    await boneCard.click();
    await page.waitForTimeout(800);

    // Verify DOM theme attribute switched to studio-bone
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");
    await expect(htmlElement).toHaveClass(/studio-bone/);

    // Take screenshot in Bone White
    const boneWhitePath = path.join(ARTIFACT_DIR, "bone_white_e2e.png");
    await page.screenshot({ path: boneWhitePath, fullPage: true });

    // 5. Switch to "Xavier Cyberpunk"
    const cyberpunkCard = page.locator('div[role="button"]:has-text("Xavier Cyberpunk")').first();
    await cyberpunkCard.click();
    await page.waitForTimeout(800);
    await expect(htmlElement).toHaveAttribute("data-theme", "cyberpunk");
    await expect(htmlElement).toHaveClass(/cyberpunk/);

    // Take screenshot in Cyberpunk
    const cyberpunkPath = path.join(ARTIFACT_DIR, "cyberpunk_e2e.png");
    await page.screenshot({ path: cyberpunkPath, fullPage: true });

    // Switch back to Studio Dark (flagship default)
    const studioDarkCard = page.locator('div[role="button"]:has-text("Studio Dark")').first();
    await studioDarkCard.click();
    await page.waitForTimeout(600);
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    // Close modal
    await modal.click();
    await page.waitForTimeout(400);
  });
});
