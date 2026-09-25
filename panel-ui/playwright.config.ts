import { defineConfig, devices } from "@playwright/test";

const baseURL = process.env.PANEL_UI_BASE_URL ?? "http://127.0.0.1:4174";

export default defineConfig({
	testDir: "./tests",
	testMatch: "e2e/**/*.spec.ts",
	// auth-2fa-real-backend.spec.ts needs a real `xavier http` process on its own port/data
	// dir (see playwright.auth-e2e.config.ts) — this config's webServer only starts `vite
	// preview` with no backend, so that spec belongs to the dedicated config exclusively.
	testIgnore: "e2e/auth-2fa-real-backend.spec.ts",
	timeout: 60_000,
	expect: {
		timeout: 15_000,
	},
	retries: process.env.CI ? 2 : 0,
	fullyParallel: false,
	reporter: [["list"]],
	use: {
		baseURL,
		actionTimeout: 10_000,
		viewport: { width: 1280, height: 720 },
		trace: "on-first-retry",
		screenshot: "only-on-failure",
		video: "retain-on-failure",
	},
	projects: [
		{
			name: "chromium",
			use: { ...devices["Desktop Chrome"] },
		},
	],
	webServer: {
		command: "npx vite preview --port 4174 --host 127.0.0.1",
		url: baseURL,
		reuseExistingServer: !process.env.CI,
		timeout: 60_000,
	},
});
