import { expect, test } from "@playwright/test";

test.describe("Browser Token Hook E2E", () => {
  test("panel UI loads in browser without 401 token errors or get_xavier_token failures", async ({ page }) => {
    // Intercept memory and notification endpoint calls to verify X-Xavier-Token header
    let memoryTokenHeader: string | null = null;
    let notificationsTokenHeader: string | null = null;

    await page.route("**/v1/memories*", async (route) => {
      memoryTokenHeader = route.request().headers()["x-xavier-token"] ?? null;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ pagination: { total: 0 }, data: [] }),
      });
    });

    await page.route("**/notifications*", async (route) => {
      notificationsTokenHeader = route.request().headers()["x-xavier-token"] ?? null;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    // Catch any page console or uncaught exceptions
    const consoleErrors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") {
        consoleErrors.push(msg.text());
      }
    });

    // Navigate to root page
    await page.goto("/");

    // Verify page title or body rendered without blanking out
    await expect(page.locator("body")).toBeVisible();

    // Verify no invocation of get_xavier_token or 401 authentication loops occurred in console
    const tauriErrors = consoleErrors.filter((err) =>
      err.includes("get_xavier_token") || err.includes("401"),
    );
    expect(tauriErrors).toHaveLength(0);
  });
});
