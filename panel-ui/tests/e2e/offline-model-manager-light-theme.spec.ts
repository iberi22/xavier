import { expect, test } from "@playwright/test";

test.describe("OfflineModelManager Light Theme Verification", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept health and auth requests
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
        body: JSON.stringify({
          access_token: "mock.jwt.access-token",
          refresh_token: "mock-refresh-token",
        }),
      });
    });

    await page.route("**/panel/api/offline-models/status", async (route) => {
        await route.fulfill({
            status: 200,
            contentType: "application/json",
            body: JSON.stringify({
                engine_status: "running",
                port: 11434,
                gpu_detected: true,
                vram_mb: 8192,
                gpu_vendor: "Apple M3 Max"
            })
        })
    });

    await page.route("**/panel/api/offline-models/config", async (route) => {
        await route.fulfill({
            status: 200,
            contentType: "application/json",
            body: JSON.stringify({
                local_dirs: ["/Users/mock/models"],
                auto_start_engine: false
            })
        })
    });

    await page.route("**/panel/api/offline-models/discover", async (route) => {
        await route.fulfill({
            status: 200,
            contentType: "application/json",
            body: JSON.stringify({
                models: [
                    {
                        path: "/Users/mock/models/Llama-2-7b-chat.gguf",
                        name: "Llama-2-7b-chat",
                        size_bytes: 4000000000,
                        quantization: "Q4_K_M"
                    }
                ]
            })
        })
    });

    // Add missing routes from antigravity
    await page.route("**/panel/api/providers/scan", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          local: { name: "local", configured: true, running: true },
          openai: { name: "openai", configured: false, running: false },
        }),
      });
    });

    await page.route("**/panel/api/providers/quota", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([
            {
                provider: "local",
                weekly_quota: 1000000,
                used_today: 0,
                used_weekly: 0,
                rate_limited_until: null
            }
        ]),
      });
    });

    await page.route("**/panel/api/providers/config", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ providers: [] }),
      });
    });

    // Bypass onboarding and set authenticated session with LIGHT mode
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.localStorage.setItem("vite-ui-theme", "light");
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

  test("renders light mode correctly, then tests dark mode", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    // Navigate to root to load the layout properly, then open modal
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible({ timeout: 15000 });
    await brainButton.click();

    const closeModalBtn = page.locator('button[aria-label="Cerrar ventana de configuración"]');
    await expect(closeModalBtn).toBeVisible({ timeout: 15000 });

    // Switch to Models tab
    const modelsSidebarBtn = page.getByTestId("settings-tab-providers");
    await expect(modelsSidebarBtn).toBeVisible({ timeout: 10000 });
    await modelsSidebarBtn.click();

    // Wait for the Offline Model Manager container to appear
    const offlineManagerHeading = page.locator('h3:has-text("Offline Model Manager")');
    await expect(offlineManagerHeading).toBeVisible({ timeout: 10000 });

    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    const offlineManagerContainer = offlineManagerHeading.locator('xpath=./ancestor::div[contains(@class, "border-")]').first();

    // The surface card should be light now (luminance check)
    // To do this we inject a bit of JS to check the computed bg color
    const luminance = await offlineManagerContainer.evaluate((node) => {
        const bg = window.getComputedStyle(node).backgroundColor; // e.g. rgb(255, 255, 255)
        const rgb = bg.match(/\d+/g)?.map(Number);
        if (!rgb || rgb.length < 3) return 0;
        // Simple relative luminance formula
        const [r, g, b] = rgb.map(c => {
            c = c / 255;
            return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
        });
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    });

    expect(luminance).toBeGreaterThan(0.7); // Light background

    // Check heading color (should be dark)
    const textColor = await offlineManagerHeading.evaluate((node) => {
        const color = window.getComputedStyle(node).color;
        const rgb = color.match(/\d+/g)?.map(Number);
        if (!rgb || rgb.length < 3) return 0;
        const [r, g, b] = rgb.map(c => {
            c = c / 255;
            return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
        });
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    });
    expect(textColor).toBeGreaterThan(0.3); // Check text

    await page.screenshot({ path: testInfo.outputPath("offline-model-manager-light.png"), fullPage: true });

    // Switch to Appearance tab to change to Dark Mode
    const appearanceSidebarBtn = page.getByTestId("settings-tab-appearance");
    await expect(appearanceSidebarBtn).toBeVisible({ timeout: 10000 });
    await appearanceSidebarBtn.click();

    // Switch to Dark Mode (Moon icon)
    const moonBtn = page.locator('button[aria-label="Dark theme"]');
    await expect(moonBtn).toBeVisible({ timeout: 10000 });
    await moonBtn.click();
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");

    // Switch back to Models tab
    await modelsSidebarBtn.click();
    await expect(offlineManagerHeading).toBeVisible({ timeout: 10000 });

    const darkLuminance = await offlineManagerContainer.evaluate((node) => {
        const bg = window.getComputedStyle(node).backgroundColor;
        const rgb = bg.match(/\d+/g)?.map(Number);
        if (!rgb || rgb.length < 3) return 0;
        const [r, g, b] = rgb.map(c => {
            c = c / 255;
            return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
        });
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    });

    expect(darkLuminance).toBeGreaterThan(0.2); // Check dark

    await page.screenshot({ path: testInfo.outputPath("offline-model-manager-dark.png"), fullPage: true });

    expect(pageErrors).toEqual([]);
  });
});
