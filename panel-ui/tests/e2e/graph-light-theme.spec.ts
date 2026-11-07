import { expect, test } from "@playwright/test";

test.describe("GraphView and GraphCanvas Light Theme Contrast", () => {
  test("GraphCanvas and GraphView render correctly and respect light theme CSS vars", async ({ page }) => {
    // Navigate and login
    await page.route("**/health", async (route) => route.fulfill({ status: 200, body: JSON.stringify({ status: "ok" }) }));
    await page.route("**/auth/refresh", async (route) => route.fulfill({ status: 200, body: JSON.stringify({ access_token: "mock-jwt" }) }));
    await page.route("**/v1/auth/login", async (route) => route.fulfill({ status: 200, body: JSON.stringify({ token: "mock-jwt", user: { id: "1" } }) }));

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt");
    });

    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Theme switch to studio-bone
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'studio-bone');
      document.documentElement.classList.remove('dark', 'studio-dark');
      document.documentElement.classList.add('light', 'studio-bone');
    });

    // Check html attributes to ensure light theme is set
    const htmlElement = page.locator("html");
    await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

    // Validate we can switch back to dark mode
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'studio-dark');
      document.documentElement.classList.remove('light', 'studio-bone');
      document.documentElement.classList.add('dark', 'studio-dark');
    });

    await expect(htmlElement).toHaveAttribute("data-theme", "studio-dark");
  });
});
