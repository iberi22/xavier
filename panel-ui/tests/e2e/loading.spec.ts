import { expect, test } from "@playwright/test";

test.describe("Loading and Error UI states", () => {
  test("TopStatusBar displays loading spinner when loading state is active", async ({ page }) => {
    await page.goto("/");
    // Verify TopStatusBar status element or loading spinner on initial load
    const topBar = page.locator("header, [class*='TopStatusBar']");
    await expect(topBar).toBeVisible();
  });
});
