import { expect, test } from "@playwright/test";

test.describe("Desktop Offline Fallback", () => {
  test("displays offline screen when health check fails in Tauri environment", async ({ page }) => {
    // 1. Mock the /health check to simulate connection refused (abort).
    await page.route("**/health", async (route) => {
      await route.abort("failed");
    });

    // 2. Inject __TAURI_INTERNALS__ so that the app believes it's in the desktop app.
    await page.addInitScript(() => {
      // By putting this in the global scope before the app loads,
      // `typeof window !== "undefined" && "__TAURI_INTERNALS__" in window`
      // will evaluate to true.
      (window as any).__TAURI_INTERNALS__ = {};
    });

    // 3. Load the app
    await page.goto("/");

    // 4. Assert that the offline UI elements render
    // Wait for the "Xavier Node Connection" header
    const header = page.locator("h1:has-text('Xavier Node Connection')");
    await expect(header).toBeVisible({ timeout: 10000 });

    // Assert the text about connecting to local daemon / pairing remote / sandbox mode
    const description = page.locator("text=Connect to your local daemon (:8006), pair a remote SWAL node, or explore the memory console in browser mode.");
    await expect(description).toBeVisible();

    // Assert the "Retry Local Connection" button exists
    const retryButton = page.locator("button:has-text('Retry Local Connection (:8006)')");
    await expect(retryButton).toBeVisible();

    // Assert the "Enter Browser Sandbox Mode" button exists
    const sandboxButton = page.locator("button:has-text('Enter Browser Sandbox Mode')");
    await expect(sandboxButton).toBeVisible();
  });
});
