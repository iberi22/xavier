import { defineConfig, devices } from "@playwright/test";

const baseURL = process.env.PANEL_UI_BASE_URL ?? "http://127.0.0.1:4174";

export default defineConfig({
	testDir: "./tests",
	testMatch: "e2e/**/*.spec.ts",
	timeout: 60_000,
	expect: {
		timeout: 15_000,
	},
	fullyParallel: false,
	reporter: [["list"]],
	use: {
		baseURL,
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
