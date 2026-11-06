import { test, expect } from "@playwright/test";

// Screenshots go under Playwright's per-test output dir (testInfo.outputPath), never the repo.

test.describe("Telecom audit trail route (#/telecom/audit-trail)", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept API routes
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "ok" } });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
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

    // AuthProvider calls authClient.refresh() on every mount (panel-ui/src/auth/AuthProvider.tsx);
    // this test navigates straight to a gated route (#/telecom/audit-trail) with no explicit login
    // click, so isAuthenticated only ever becomes true via this call. Shape matches the real
    // backend (refresh_handler's AuthResponse, src/auth2/mod.rs): { access_token, refresh_token }.
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock.jwt.access-token-audit-trail",
          refresh_token: "mock-refresh-token-audit-trail",
        }),
      });
    });

    // Bypass onboarding and set authenticated session
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
  });

  test("first page load renders the styled audit trail with an honest empty state", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    // Direct first load of the hash route: no prior navigation, no injected markup.
    await page.goto("/#/telecom/audit-trail");

    const heading = page.getByRole("heading", { name: "Telecom Clearance & Security Audit Trail" });
    await expect(heading).toBeVisible();
    const metrics = page.getByRole("region", { name: "Security Metrics Summary" });
    await expect(metrics).toContainText("Total Events");

    const timeline = page.getByRole("log", { name: "Security verification timeline log" });
    await expect(timeline).toContainText("No telecom audit events have been recorded yet.");
    // No demo events are presented as real history on this route.
    await expect(page.getByRole("button", { name: /View security audit event details/ })).toHaveCount(0);

    // Panel styling comes from the app's Tailwind build (dark slate surface), not bare HTML.
    const surface = heading.locator("xpath=ancestor::div[contains(@class,'bg-slate-950')][1]");
    await expect(surface).toHaveCSS("background-color", /oklch\(0\.129 0\.042 264\.695\)|rgb\(2, 6, 23\)/);

    // Filters and search stay usable on an empty feed.
    const blocked = page.getByRole("button", { name: /Blocked \(0\)/ });
    await expect(blocked).toBeVisible();
    await blocked.click();
    await page.getByPlaceholder("Filter by participant, channel, action, or details...").fill("node-alice");
    await expect(timeline).toContainText("No telecom audit events have been recorded yet.");

    await page.screenshot({ path: testInfo.outputPath("telecom_audit_trail_route.png"), fullPage: true });
    expect(pageErrors).toEqual([]);
  });

  test("back button returns to the telecom hub", async ({ page }) => {
    await page.goto("/#/telecom/audit-trail");
    await page.getByRole("button", { name: "Back to previous view" }).click();
    await expect(page).toHaveURL(/#\/telecom$/);
    await expect(page.getByRole("tab", { name: /Network Peers/ })).toBeVisible();
  });
});
