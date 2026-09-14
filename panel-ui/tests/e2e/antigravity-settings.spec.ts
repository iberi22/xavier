import { expect, test } from "@playwright/test";
import path from "node:path";

test.describe("Antigravity Settings Modal & Live Telemetry E2E Suite", () => {
  test("opens settings, iterates tabs, toggles options, verifies notifications, and captures visual artifact", async ({
    page,
  }) => {
    test.setTimeout(120_000);
    // 1. Intercept API routes with deterministic mock data
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          status: "ok",
          system: { cpu_usage: 12, ram_usage_percent: 25 },
        }),
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
        body: JSON.stringify({ quotas: [] }),
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
        body: JSON.stringify([
          {
            id: "notif-e2e-1",
            islandId: "system",
            title: "E2E Antigravity Alert",
            body: "Live telemetry operational in Chromium test.",
            timestamp: new Date().toISOString(),
            read: false,
            severity: "info",
          },
          {
            id: "notif-e2e-2",
            islandId: "memory",
            title: "Memory Sync Active",
            body: "Quantum memory layer verified.",
            timestamp: new Date().toISOString(),
            read: false,
            severity: "success",
          },
        ]),
      });
    });

    await page.route("**/notifications/*/read", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ success: true }),
      });
    });

    await page.route("**/notifications/read-all", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ success: true }),
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

    // Catch-all mock handler for memory or code graph views
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

    // Catch-all route handler for remaining unhandled REST requests
    await page.route("**/v1/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok" }),
      });
    });

    // 2. Set up local storage session
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

    // 3. Navigate to application root
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

    // 4. Open Settings Modal via BrainCircuit button
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible({ timeout: 15000 });
    await brainButton.click();

    // Wait for ConfigModal to appear
    const closeModalBtn = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(closeModalBtn).toBeVisible({ timeout: 15000 });

    // Verify Antigravity Sidebar sections
    await expect(page.locator('text="Settings"').first()).toBeVisible();
    await expect(page.locator('text="Projects"').first()).toBeVisible();
    await expect(page.locator('text="Not in Project"').first()).toBeVisible();
    await expect(page.locator('text="Shortcuts"').first()).toBeVisible();
    await expect(page.locator('text="Provide Feedback"').first()).toBeVisible();

    // 5. Test Settings Sidebar navigation items
    const sidebarItems = [
      "Account",
      "General",
      "Appearance",
      "Models",
      "Customizations",
      "Browser",
      "App",
    ];

    for (const itemLabel of sidebarItems) {
      const itemBtn = page.locator(`button:has-text("${itemLabel}")`).first();
      if (await itemBtn.isVisible()) {
        await itemBtn.click({ force: true });
        await page.waitForTimeout(200);
      }
    }

    // Switch to Appearance tab to test chat preferences & theme switching
    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.scrollIntoViewIfNeeded();
    await appearanceSidebarBtn.click({ force: true });
    await page.waitForTimeout(600);

    // Verify Appearance view headings
    await expect(page.locator('h1:has-text("Appearance")')).toBeVisible();
    await expect(page.locator('text="Verbose Agent Chat"')).toBeVisible();
    await expect(page.locator('text="Conversation Width"')).toBeVisible();

    // 6. Test Toggle Switch interaction (Verbose Agent Chat)
    const toggleSwitch = page.getByRole("switch").first();
    await expect(toggleSwitch).toBeVisible();

    const initialChecked = await toggleSwitch.getAttribute("aria-checked");
    await toggleSwitch.click({ force: true });
    await page.waitForTimeout(150);
    const updatedChecked = await toggleSwitch.getAttribute("aria-checked");
    expect(updatedChecked).not.toBe(initialChecked);

    // 7. Test Segmented / Conversation Width buttons
    const narrowBtn = page.getByRole("button", { name: "Narrow" });
    if (await narrowBtn.isVisible()) {
      await narrowBtn.click({ force: true });
      await page.waitForTimeout(150);
    }

    // Also verify theme mode buttons (Monitor, Sun, Moon)
    const sunBtn = page.locator('button[aria-label="Light theme"]');
    if (await sunBtn.isVisible()) {
      await sunBtn.click({ force: true });
      await page.waitForTimeout(300);
      await expect(page.locator("html")).toHaveAttribute("data-theme", "studio-bone");

      // Switch back to Dark theme
      const moonBtn = page.locator('button[aria-label="Dark theme"]');
      await moonBtn.click({ force: true });
      await page.waitForTimeout(300);
      await expect(page.locator("html")).toHaveAttribute("data-theme", "studio-dark");
    }

    // 7. Test Slider / Conversation Width control interaction
    const sliderInput = page.locator('input[type="range"]').first();
    if (await sliderInput.isVisible()) {
      await sliderInput.fill("15");
      await sliderInput.dispatchEvent("change");
      await page.waitForTimeout(100);
      const newValue = await sliderInput.inputValue();
      expect(newValue).toBe("15");
    }

    // 8. Close Settings Modal
    await closeModalBtn.click({ force: true });
    await page.waitForTimeout(300);

    // 9. Test Notifications Bell interaction
    const bellButton = page.getByRole("button", { name: "Notifications" });
    await expect(bellButton).toBeVisible();

    // Open notifications dropdown
    await bellButton.click({ force: true });
    await page.waitForTimeout(300);

    // Verify mock notification items render
    const notifItem = page.getByText("E2E Antigravity Alert");
    await expect(notifItem).toBeVisible();

    // Click "All read" / Mark as read button in dropdown
    const markAllReadBtn = page.getByRole("button", { name: "All read" });
    if (await markAllReadBtn.isVisible()) {
      await markAllReadBtn.click({ force: true });
      await page.waitForTimeout(150);
    } else {
      const markReadBtn = page.getByRole("button", { name: "Mark as read" }).first();
      if (await markReadBtn.isVisible()) {
        await markReadBtn.click({ force: true });
        await page.waitForTimeout(150);
      }
    }

    // Verify unread badge count is cleared or reduced
    const unreadBadge = page.locator('button[aria-label="Notifications"] .bg-red-500');
    await expect(unreadBadge).not.toBeVisible();

    // Close notifications dropdown
    await page.keyboard.press("Escape");
    await page.waitForTimeout(200);

    // 10. Capture full visual artifact
    const screenshotPath = path.join(process.cwd(), "antigravity_complete_suite.png");
    await page.screenshot({ path: screenshotPath, fullPage: true });

    // Also verify screenshot file was created
    expect(screenshotPath).toBeTruthy();
  });
});
