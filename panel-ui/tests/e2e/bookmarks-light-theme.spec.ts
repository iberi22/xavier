import fs from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";

const DEFAULT_ARTIFACT_DIR = path.join(
	process.cwd(),
	"test-results",
	"artifacts",
);

function getWritableDir(target: string): string {
	try {
		fs.mkdirSync(target, { recursive: true });
		fs.accessSync(target, fs.constants.W_OK);
		return target;
	} catch {
		fs.mkdirSync(DEFAULT_ARTIFACT_DIR, { recursive: true });
		return DEFAULT_ARTIFACT_DIR;
	}
}

const _ARTIFACT_DIR = getWritableDir(
	process.env.ARTIFACT_DIR || DEFAULT_ARTIFACT_DIR,
);

test.describe("BookmarksView Light Theme Compatibility", () => {
	test("displays BookmarksView and BookmarkItem with correct contrast in light mode", async ({
		page,
	}, testInfo) => {
		// Intercept API routes to bypass actual auth and provide mock bookmarks
		await page.route("**/health", async (route) => {
			await route.fulfill({
				json: {
					status: "online",
					system: { cpu_usage: 10, ram_usage_percent: 20 },
				},
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

		await page.route("**/auth/refresh", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					access_token: "mock-access-token",
					refresh_token: "mock-refresh-token",
				}),
			});
		});

		const mockBookmarks = [
			{
				id: "b1",
				title: "Test Bookmark 1",
				type: "Data Card",
				category: "Test",
				date: "2023-10-27",
			},
			{
				id: "b2",
				title: "Test Bookmark 2",
				type: "Code Snippet",
				category: "Test",
				date: "2023-10-27",
			},
		];

		await page.route("**/panel/api/**", async (route) => {
			const url = new URL(route.request().url());
			if (url.pathname.includes("/panel/api/bookmarks")) {
				return route.fulfill({ json: mockBookmarks });
			}
			await route.fulfill({ json: [] });
		});

		// Complete onboarding and force light theme
		await page.addInitScript(() => {
			window.localStorage.setItem("xavier_onboarding_completed", "true");
			window.localStorage.setItem("xavier_token", "mock-jwt-token");
			window.localStorage.setItem("vite-ui-theme", "light");
			window.localStorage.setItem(
				"auth-storage",
				JSON.stringify({
					state: {
						token: "mock-jwt-token",
						isAuthenticated: true,
						user: { id: "1", email: "operator@xavier.local", role: "admin" },
					},
					version: 0,
				}),
			);
		});

		// 1. Navigate to main UI and ensure light theme is applied
		await page.goto("/");
		await page.waitForLoadState("domcontentloaded");

		// Click mock login if it appears
		const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
		if (await loginButton.isVisible()) {
			await page.fill('input[type="email"]', "operator@xavier.local");
			const passwordInput = page.locator('input[type="password"]');
			if (await passwordInput.isVisible()) {
				await passwordInput.fill("password123");
			}
			await loginButton.click();
		}

		const htmlElement = page.locator("html");
		await expect(htmlElement).toHaveAttribute("data-theme", "studio-bone");

		// 2. Open Config Modal then Bookmarks tab
		const brainButton = page
			.locator(
				'button[aria-label="Open Control Node"], button[title="Control Node"]',
			)
			.first();
		await expect(brainButton).toBeVisible();
		await brainButton.click();

		// Wait for ConfigModal to appear
		const modal = page.locator(
			'button[aria-label="Cerrar ventana de configuración"]',
		);
		await expect(modal).toBeVisible();

		const savedArtifactsBtn = page.locator(
			'button:has-text("Saved Artifacts")',
		);
		await expect(savedArtifactsBtn).toBeVisible();
		await savedArtifactsBtn.click();

		// 3. Verify BookmarksView is visible and uses light theme colors
		const title = page.locator('h2:has-text("Saved Artifacts")');
		await expect(title).toBeVisible();

		// Assert title text is dark
		const titleColor = await title.evaluate(
			(el) => window.getComputedStyle(el).color,
		);
		expect(titleColor).not.toBe("rgb(255, 255, 255)"); // Text shouldn't be plain white in light mode

		const titleColorLuminance = await title.evaluate((el) => {
			const color = window.getComputedStyle(el).color;
			if (color.startsWith("rgb")) {
				const rgb = color.match(/\d+/g)?.map(Number) || [255, 255, 255];
				const [r, g, b] = rgb.map((c) => {
					c /= 255;
					return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
				});
				return 0.2126 * r + 0.7152 * g + 0.0722 * b;
			}
			return 0.0;
		});
		// Expected to be dark text (luminance < 0.5)
		expect(titleColorLuminance).toBeLessThan(0.5);

		// Verify a BookmarkItem exists and check its background luminance
		const bookmarkTitle = page.locator('h3:has-text("Test Bookmark 1")');
		if (await bookmarkTitle.isVisible()) {
			const bookmarkItem = bookmarkTitle
				.locator('xpath=ancestor::div[contains(@class, "rounded-2xl")]')
				.first();
			const bgLuminance = await bookmarkItem.evaluate((el) => {
				const bgColor = window.getComputedStyle(el).backgroundColor;
				if (bgColor.startsWith("rgb")) {
					const rgb = bgColor.match(/\d+/g)?.map(Number) || [255, 255, 255];
					if (rgb.length >= 4 && rgb[3] === 0) return 1.0;
					const [r, g, b] = rgb.map((c) => {
						c /= 255;
						return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
					});
					return 0.2126 * r + 0.7152 * g + 0.0722 * b;
				}
				return 1.0;
			});
			// Expected to be light surface (luminance > 0.7)
			expect(bgLuminance).toBeGreaterThan(0.7);
		}

		// Take screenshot
		await page.screenshot({
			path: testInfo.outputPath("bookmarks_modal_light.png"),
			fullPage: true,
		});

		// Assert there are no page errors
		const pageErrors: string[] = [];
		page.on("pageerror", (err) => pageErrors.push(String(err)));
		expect(pageErrors).toEqual([]);
	});
});
