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
	// No backend is started here ON PURPOSE: every API call in this suite is mocked with
	// page.route(), and vite.config.ts sets `preview.proxy = {}` so nothing can leak to a
	// daemon on :8006 (on a production node that would be the live instance). Specs that need
	// a real backend run from playwright.auth-e2e.config.ts, which boots a disposable
	// `xavier http` on its own port (18016) and temp data dir.
	webServer: {
		command: "npx vite preview --port 4174 --host 127.0.0.1",
		url: baseURL,
		reuseExistingServer: !process.env.CI,
		timeout: 60_000,
	},
});
