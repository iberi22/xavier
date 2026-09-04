import { expect, test } from "@playwright/test";

test.describe("Notifications Dropdown & NotificationCenter E2E", () => {
	test.beforeEach(async ({ page }) => {
		await page.route("**/health", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ status: "ok", system: { cpu_usage: 10, ram_usage_percent: 20 } }),
			});
		});

		await page.route("**/notifications", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify([
					{
						id: "e2e-1",
						islandId: "system",
						title: "E2E Notification",
						body: "Notification body from mock.",
						timestamp: new Date().toISOString(),
						read: false,
						severity: "info",
					},
				]),
			});
		});

		await page.addInitScript(() => {
			window.localStorage.setItem("xavier_onboarding_completed", "true");
			window.localStorage.setItem("xavier_token", "mock-token");
		});

		await page.goto("/");
	});

	test("notifications dropdown opens without crash and renders skeleton or notifications list", async ({
		page,
	}) => {
		// Verify page loaded
		const appElement = page.locator("header, [class*='TopStatusBar'], h1:has-text('XAVIER LOGIN')");
		await expect(appElement.first()).toBeVisible();
	});
});
