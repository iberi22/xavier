import { expect, test } from "@playwright/test";

test.describe("Xavier Studio Dark & Antigravity Appearance E2E Verification", () => {
  test("loads Studio Dark by default, captures screenshots, and verifies theme switching", async ({
    page,
  }, testInfo) => {
    // Intercept health and auth requests
    await page.route("**/v1/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok" }),
      });
    });

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

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ token: "mock-jwt-token" }),
      });
    });

    await page.route("**/v1/config/providers", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          providers: [
            { provider: "openai", api_key: "sk-mock", model: "gpt-4o" },
            { provider: "gemini", api_key: "mock-gemini", model: "gemini-1.5-pro" },
          ],
        }),
      });
    });

    await page.route("**/v1/account/usage", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          status: "ok",
          requests_used: 10,
          total_tokens: 1000,
          total_errors: 0,
          total_cost_usd: 0.001,
          memory_fallback_hits: 2,
          fallback_chain_hops: 0,
          by_provider: {
            openai: { requests: 10, tokens: 1000, errors: 0, cost_usd: 0.001 },
          },
        }),
      });
    });

    await page.route("**/v1/providers/quota", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    await page.route("**/v1/system/scan", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok" }),
      });
    });

    await page.route("**/v1/memories*", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ data: [], pagination: { total: 0 } }),
      });
    });

    await page.route("**/api/memory/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ results: [], data: [] }),
      });
    });

    await page.route("**/api/agents**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ agents: [] }),
      });
    });

    await page.route("**/notifications", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    await page.route("**/panel/api/threads", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    await page.route("**/panel/api/bookmarks", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    await page.route("**/panel/api/widgets", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    await page.route("**/panel/api/graph", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ id: "default", name: "Roadmap", data: { nodes: [], links: [] } }),
      });
    });

    await page.route("**/memory/graph/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ nodes: [], edges: [], links: [] }),
      });
    });

    await page.route("**/code/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ total_symbols: 0, total_files: 0, nodes: [], links: [] }),
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

    // If login is shown, log in
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      const passwordInput = page.locator('input[type="password"]');
      if (await passwordInput.isVisible()) {
        await passwordInput.fill("password123");
      }
      await loginButton.click();
      await expect(loginButton).not.toBeVisible();
    }

    // 2. Verify Studio Dark is applied by default
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");
    await expect(htmlElement).toHaveClass(/dark/);
    await expect(htmlElement).toHaveClass(/studio-dark/);

    // Take screenshot of main shell in Studio Dark
    const studioDarkPath = testInfo.outputPath("studio_dark_e2e.png");
    await page.screenshot({ path: studioDarkPath, fullPage: true });

    // 3. Open Config Modal to inspect Appearance tab
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible({ timeout: 15000 });
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
    await expect(page.locator('h1:has-text("Appearance")')).toBeVisible();

    // Verify Antigravity Appearance content
    await expect(page.locator('text="Verbose Agent Chat"')).toBeVisible();
    await expect(page.locator('text="Conversation Width"')).toBeVisible();
    await expect(page.locator('text="Light Theme"')).toBeVisible();
    await expect(page.locator('text="Dark Theme"')).toBeVisible();

    // Capture antigravity_modal_aligned.png in caller artifact dir
    const antigravityAlignedPath = testInfo.outputPath("antigravity_modal_aligned.png");
    await page.screenshot({ path: antigravityAlignedPath, fullPage: true });

    // Also update appearance_modal_e2e.png
    const appearanceModalPath = testInfo.outputPath("appearance_modal_e2e.png");
    await page.screenshot({ path: appearanceModalPath, fullPage: true });

    // 4. Switch to Light Mode (Sun icon)
    const sunBtn = page.locator('button[aria-label="Light theme"]');
    await sunBtn.click();

    // Verify DOM theme attribute switched to studio-bone
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");
    await expect(htmlElement).toHaveClass(/studio-bone/);

    // Take screenshot in Bone White
    const boneWhitePath = testInfo.outputPath("bone_white_e2e.png");
    await page.screenshot({ path: boneWhitePath, fullPage: true });

    // 5. Switch to Dark Mode (Moon icon)
    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await moonBtn.click();
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");
    await expect(htmlElement).toHaveClass(/studio-dark/);

    // Close modal
    await modal.click();
    await expect(modal).not.toBeVisible();
  });
});

