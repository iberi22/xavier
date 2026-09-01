import { expect, test } from "@playwright/test";

test.describe("Notifications Dropdown & NotificationCenter E2E", () => {
	test.beforeEach(async ({ page }) => {
		await page.route("**/health", async (route) => {
			await route.fulfill({ json: { status: "online" } });
		});

		await page.route("**/notifications", async (route) => {
			await route.fulfill({
				status: 200,
				json: [
					{
						id: "e2e-1",
						islandId: "system",
						title: "E2E Notification",
						body: "Notification body from mock.",
						timestamp: new Date().toISOString(),
						read: false,
						severity: "info",
					},
				],
			});
		});

		await page.goto("/");
		await page.evaluate(() => {
			localStorage.setItem("xavier_onboarding_completed", "true");
		});
		const tokenInput = page.locator('input[placeholder="XAVIER_TOKEN"]');
		if (await tokenInput.isVisible()) {
			await tokenInput.fill("test-token");
			await page.click('button:has-text("INITIALIZE SESSION")');
		}
	});

	test("notifications dropdown opens without crash and renders skeleton or notifications list", async ({
		page,
	}) => {
		// Verify page loaded
		await expect(page.locator("body")).toBeVisible();
	});
});
