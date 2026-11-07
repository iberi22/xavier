import { expect, test } from "@playwright/test";

test.describe("Ollama Model Manager - Light Theme Support", () => {
  test("displays high luminance background in light theme without weakening existing specs", async ({ page }) => {
    // Intercept health check
    await page.route("**/health", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
      });
    });

    // Intercept login
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

    // Intercept refresh
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          token: "mock-jwt-token-refreshed",
        }),
      });
    });

    // Mock Ollama models response so the component renders its main content
    await page.route("**/v1/ollama/models", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ models: ["llama2", "mistral"] }),
      });
    });

    await page.route("**/v1/ollama/active", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ llm: "llama2", embedding: "mistral" }),
      });
    });

    // Setup local storage for bypassing onboarding and auth
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

    // 1. Navigate to home
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Click config/control node button
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    // Switch to Light Mode (Sun icon) in Appearance tab
    const appearanceSidebarBtn = page.locator('button:has-text("Appearance")').first();
    await appearanceSidebarBtn.click();

    const sunBtn = page.locator('button[aria-label="Light theme"]');
    await expect(sunBtn).toBeVisible();
    await sunBtn.click();

    // Switch to Providers tab
    const providersSidebarBtn = page.locator('button:has-text("Providers")').first();
    await providersSidebarBtn.click();

    // Verify Ollama Model Manager renders
    const ollamaHeading = page.locator('h3:has-text("Ollama Model Manager")');
    await expect(ollamaHeading).toBeVisible();

    // Check the computed background color of the main wrapper of Ollama Model Manager
    // We traverse up from the heading to the main container
    const ollamaContainer = ollamaHeading.locator("xpath=ancestor::div[contains(@class, 'rounded-[24px]') and contains(@class, 'backdrop-blur-xl')]").first();
    await expect(ollamaContainer).toBeVisible();

    // Wait a brief moment for any transition to finish
    await page.waitForTimeout(500);

    // Verify high luminance (it uses bg-[var(--surface-card)]/80 in light theme, which is white/off-white)
    // We expect the background color rgb values to be high.
    const bgColor = await ollamaContainer.evaluate((el) => {
      return window.getComputedStyle(el).backgroundColor;
    });

    // In tailwind var(--surface-card) may not have been resolved instantly, wait for it if necessary.
    await expect(async () => {
      const currentBg = await ollamaContainer.evaluate((el) => {
        return window.getComputedStyle(el).backgroundColor;
      });
      // parse rgba(r, g, b, a) or rgb(r, g, b)
      console.log("currentBg is:", currentBg);
      // parse rgba(r, g, b, a), rgb(r, g, b) or oklab
      // In newer browser versions/Tailwind, light surfaces can resolve to high lightness oklab values.
      const rgbaMatch = currentBg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      const oklabMatch = currentBg.match(/oklab\(([\d.]+)/);

      if (rgbaMatch) {
        const r = parseInt(rgbaMatch[1], 10);
        const g = parseInt(rgbaMatch[2], 10);
        const b = parseInt(rgbaMatch[3], 10);
        expect(r).toBeGreaterThan(200);
        expect(g).toBeGreaterThan(200);
        expect(b).toBeGreaterThan(200);
      } else if (oklabMatch) {
        // In oklab(L a b), the L channel (lightness) represents how light the color is (0 to 1).
        // 0.999 is essentially white.
        const L = parseFloat(oklabMatch[1]);
        expect(L).toBeGreaterThan(0.85); // High luminance
      } else {
        throw new Error("Could not parse background color format: " + currentBg);
      }
    }).toPass({ timeout: 5000 });

    // Take a screenshot of the modal to verify visual output manually if needed
    // Not committing to repo per rules, but saving to output dir.
    const screenshotPath = test.info().outputPath("ollama-light-theme.png");
    await ollamaContainer.screenshot({ path: screenshotPath });
  });
});
