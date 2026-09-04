import { expect, test } from "@playwright/test";

test.describe("InputArea browser fallback E2E Tests", () => {
  test("should display FolderPlus button and handle browser file fallback without crashing", async ({
    page,
  }) => {
    // Intercept health check and auth login
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

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
    });

    // Navigate to application
    await page.goto("/");

    // Authenticate if on login page
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible()) {
      await page.fill('input[type="email"]', "operator@xavier.local");
      await page.fill('input[type="password"]', "password123");
      await loginButton.click();
    }

    // Verify FolderPlus button is visible
    const folderButton = page.locator('button[aria-label="Add project codebase"]');
    await expect(folderButton).toBeVisible();

    // Ensure hidden file input exists
    const hiddenFileInput = page.locator('input[type="file"]');
    await expect(hiddenFileInput).toBeAttached();

    // Click FolderPlus button in browser mode (triggers file input click without throwing error)
    await folderButton.click();

    // Simulate file input change event with a directory (webkitdirectory requires directory path)
    await hiddenFileInput.setInputFiles("./tests/e2e");

    // Verify that system message appears in system chat / messages
    await expect(page.locator("text=Carpeta seleccionada:")).toBeVisible();
  });
});
