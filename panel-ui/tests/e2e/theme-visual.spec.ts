import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const DEFAULT_ARTIFACT_DIR = path.join(process.cwd(), "test-results", "artifacts");

function getWritableDir(target: string): string {
  try {
    fs.mkdirSync(target, { recursive: true });
    fs.accessSync(target, fs.constants.W_OK);
    return target;
  } catch {
    fs.mkdirSync(DEFAULT_ARTIFACT_DIR, { recursive: true });
    return DEFAULT_ARTIFACT_DIR;
  }
}

const CALLER_ARTIFACT_DIR = getWritableDir(process.env.ARTIFACT_DIR || "/home/belal/.gemini/antigravity/brain/c0a3c8f4-05ed-4f34-a95f-0246711e76c1");
const ARTIFACT_DIR = getWritableDir(process.env.ARTIFACT_DIR || "/home/belal/.gemini/antigravity/brain/93991307-c184-424b-87e0-d141afcaa915");

test.describe("Xavier Studio Dark & Antigravity Appearance E2E Verification", () => {
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

    // Add missing auth/refresh intercept
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          token: "mock-jwt-token",
          user: { id: "1", email: "operator@xavier.local", role: "admin" },
        }),
      });
    });

    // Fallback v1/auth/login intercept
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

    // Add auto-wait explicitly for components to mount
    await expect(page.locator("html")).toBeVisible();

    // If login is shown, log in
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible({ timeout: 2000 }).catch(() => false)) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
    }

    await expect(page.locator('button[aria-label="Open Control Node"]')).toBeVisible();

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

    // Verify Antigravity Sidebar sections
    await expect(page.locator('text="Settings"').first()).toBeVisible();
    await expect(page.locator('text="Projects"').first()).toBeVisible();
    await expect(page.locator('text="Not in Project"').first()).toBeVisible();
    await expect(page.locator('text="Shortcuts"').first()).toBeVisible();
    await expect(page.locator('text="Provide Feedback"').first()).toBeVisible();

    // Appearance is default tab or switch to it
    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    // Verify Antigravity Appearance content
    const appearanceHeader = page.locator('h1:has-text("Appearance")');
    await expect(appearanceHeader).toBeVisible();
    await expect(page.locator('text="Verbose Agent Chat"')).toBeVisible();
    await expect(page.locator('text="Conversation Width"')).toBeVisible();
    await expect(page.locator('text="Light Theme"')).toBeVisible();
    await expect(page.locator('text="Dark Theme"')).toBeVisible();

    // Capture antigravity_modal_aligned.png in caller artifact dir
    const antigravityAlignedPath = path.join(CALLER_ARTIFACT_DIR, "antigravity_modal_aligned.png");
    await page.screenshot({ path: antigravityAlignedPath, fullPage: true });

    // Also update appearance_modal_e2e.png
    const appearanceModalPath = path.join(ARTIFACT_DIR, "appearance_modal_e2e.png");
    await page.screenshot({ path: appearanceModalPath, fullPage: true });

    // 4. Switch to Light Mode (Sun icon)
    const sunBtn = page.locator('button[aria-label="Light theme"]');
    await sunBtn.click();

    // Verify DOM theme attribute switched to studio-bone
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");
    await expect(htmlElement).toHaveClass(/studio-bone/);

    // Verify luminance (text should be dark)
    await expect(appearanceHeader).toHaveCSS("color", /rgb\(28, 28, 31\)|rgba\(28, 28, 31, 0\.95\)|oklch\(0\.208 0\.042 265\.755\)/);

    // Take screenshot in Bone White
    const boneWhitePath = path.join(ARTIFACT_DIR, "bone_white_e2e.png");
    await page.screenshot({ path: boneWhitePath, fullPage: true });

    // 5. Switch to Dark Mode (Moon icon)
    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await moonBtn.click();

    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");
    await expect(htmlElement).toHaveClass(/studio-dark/);

    // Close modal
    await modal.click();
    await expect(modal).toBeHidden();
  });
});
