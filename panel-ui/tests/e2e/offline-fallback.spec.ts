import { expect, test, type Page } from "@playwright/test";

// Ported from #3879 (targets main) and extended for integration/core-100.
// App.tsx shows the "Xavier Node Connection" screen only inside the desktop shell
// (`__TAURI_INTERNALS__` present) when GET /health fails; browsers fall back to "ok".

async function simulateDesktopShell(page: Page) {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });
}

const heading = (page: Page) => page.getByRole("heading", { name: "Xavier Node Connection" });

test.describe("Desktop offline fallback", () => {
  test("shows the connection screen when the daemon is unreachable in the desktop shell", async ({ page }, testInfo) => {
    await page.route("**/health", (route) => route.abort("connectionrefused"));
    await simulateDesktopShell(page);


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

    await page.goto("/");

    await expect(heading(page)).toBeVisible();
    await expect(
      page.getByText("Connect to your local daemon (:8006), pair a remote SWAL node, or explore the memory console in browser mode."),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Retry Local Connection (:8006)" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Enter Browser Sandbox Mode" })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("offline_fallback.png"), fullPage: true });
  });

  test("retry reconnects once the daemon is back", async ({ page }) => {
    let daemonUp = false;
    await page.route("**/health", (route) =>
      daemonUp ? route.fulfill({ json: { status: "ok" } }) : route.abort("connectionrefused"),
    );
    await simulateDesktopShell(page);

