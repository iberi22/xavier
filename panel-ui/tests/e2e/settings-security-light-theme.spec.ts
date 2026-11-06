import { test, expect } from "@playwright/test";

test.describe("Settings Security Light Theme", () => {
  test("security settings page has correct luminance in light theme", async ({ page }) => {
    // Intercept endpoints with real shapes
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "ok" } });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        json: {
          token: "mock-jwt-token"
        },
      });
    });

    // Bypass onboarding and set authenticated session + light theme
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "light");
      window.localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            token: "mock-jwt-token",
            isAuthenticated: true,
            user: { id: "1", email: "operator@xavier.local", role: "admin" }
          },
          version: 1,
        })
      );
    });

    await page.goto("/");

    // Open Settings Modal via BrainCircuit button
    const brainButton = page.locator('button[aria-label="Open Control Node"]');
    await expect(brainButton).toBeVisible();
    await brainButton.click();

    // Navigate to Security tab
    const securityTab = page.locator('button[data-testid="sidebar-btn-security"]');
    if (await securityTab.isVisible()) {
        await securityTab.click();
    } else {
        // Find by text if testid not found
        await page.locator('text="Security"').first().click();
    }


    // Wait for the page to render (the heading or token section)
    await expect(page.locator("text=API Tokens").first()).toBeVisible();

    // Verify theme attribute on html
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    // We take a screenshot
    await page.screenshot({ path: test.info().outputPath("security-light-theme.png") });

    // Assert main container luminance > 0.7 and headings dark
    // The security page has a main container that contains API Tokens.
    // Let's get the background color of the first token or main container.
    // In our component, we have a section: <div className="space-y-4">
    const tokenSection = page.locator("text=API Tokens").locator("..").locator("..");

    // Actually, just find the background of `body` or a large container and text color of heading.
    const heading = page.locator("text=API Tokens").first();

    // Evaluate luminance
    const colors = await heading.evaluate((el) => {
      const mainContainer = document.querySelector('.studio-bone') || document.body;
      const mainStyle = window.getComputedStyle(mainContainer);
      const bgMatch = mainStyle.backgroundColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      const bgLuminance = bgMatch ? (0.299 * parseInt(bgMatch[1]) + 0.587 * parseInt(bgMatch[2]) + 0.114 * parseInt(bgMatch[3])) / 255 : 0;

      const elStyle = window.getComputedStyle(el);
      const textMatch = elStyle.color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      const textLuminance = textMatch ? (0.299 * parseInt(textMatch[1]) + 0.587 * parseInt(textMatch[2]) + 0.114 * parseInt(textMatch[3])) / 255 : 1;

      return { bgLuminance, textLuminance };
    });


    console.log('Colors:', colors);
    expect(colors.bgLuminance).toBeGreaterThan(0.7);

    expect(colors.textLuminance).toBeLessThan(0.3); // "headings dark"

    // Switch to dark mode and verify it's still dark
    await page.evaluate(() => {
      window.localStorage.setItem("vite-ui-theme", "dark");
      window.dispatchEvent(new Event("storage"));
    });
    // The app might need a reload or a theme toggle click if dispatching storage event isn't enough,
    // let's try reload.
    await page.reload();
    await expect(page.locator("text=API Tokens").first()).toBeVisible();

    const darkHtmlElement = page.locator("html");
    await expect(darkHtmlElement).not.toHaveAttribute("data-theme", "studio-bone");

    const darkColors = await page.locator("text=API Tokens").first().evaluate((el) => {
      const mainContainer = document.querySelector('html') || document.body;
      const mainStyle = window.getComputedStyle(mainContainer);
      const bgMatch = mainStyle.backgroundColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      const bgLuminance = bgMatch ? (0.299 * parseInt(bgMatch[1]) + 0.587 * parseInt(bgMatch[2]) + 0.114 * parseInt(bgMatch[3])) / 255 : 1;
      return { bgLuminance };
    });

    expect(darkColors.bgLuminance).toBeLessThan(0.3);
  });
});
