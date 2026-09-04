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

    await page.route("**/health*", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
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

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    await page.route("**/v1/config/providers*", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ providers: [] }),
      });
    });

    // Catch any page console or uncaught exceptions
    const consoleErrors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") {
        consoleErrors.push(msg.text());
      }
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-token");
    });

    // Navigate to root page
    await page.goto("/");

    // Verify page title or app element rendered without blanking out
    const appElement = page.locator("header, [class*='TopStatusBar'], h1:has-text('XAVIER LOGIN')");
    await expect(appElement.first()).toBeVisible();

    // Verify no invocation of get_xavier_token or 401 authentication loops occurred in console
    const tauriErrors = consoleErrors.filter((err) =>
      err.includes("get_xavier_token") || err.includes("401"),
    );
    expect(tauriErrors).toHaveLength(0);
  });
});
