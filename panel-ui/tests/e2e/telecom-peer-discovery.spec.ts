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

const ARTIFACT_DIR = getWritableDir(
	process.env.ARTIFACT_DIR || DEFAULT_ARTIFACT_DIR,
);

test.describe("Telecom Peer Discovery E2E Verification", () => {
	test.beforeEach(async ({ page }) => {
		// Intercept health and auth requests
		await page.route("**/health", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					status: "ok",
					system: { cpu_usage: 10, ram_usage_percent: 20 },
				}),
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

		// Mock endpoints to prevent ECONNREFUSED
		await page.route("**/panel/api/threads", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/panel/api/bookmarks", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/panel/api/widgets", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/panel/api/graph", async (route) => {
			await route.fulfill({ json: { data: { nodes: [], links: [] } } });
		});
		await page.route("**/v1/config/providers", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/auth/refresh", async (route) => {
			await route.fulfill({ json: { token: "mock-jwt-token" } });
		});
		await page.route("**/notifications", async (route) => {
			await route.fulfill({ json: [] });
		});

		// Realistic MeshStatus payload (src/types.ts). `last_seen_at` is unix SECONDS, as the
		// daemon sends it (src/api/mesh.rs); one peer was seen just now, one an hour ago.
		const nowS = Math.floor(Date.now() / 1000);
		await page.route("**/v1/mesh/peers", async (route) => {
			await route.fulfill({
				json: {
					peers: [
						{
							node_id: "0x7a8f3b2c9d1e4f5a6b7c8d9e0f1a2b3c4d5e6f7a",
							alias: "Primary Gateway",
							endpoint_url: "tcp://1.2.3.4:5678",
							role: "admin",
							clearance: "secret",
							last_seen_at: nowS,
							sync_enabled: true,
						},
						{
							node_id: "0x1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e",
							alias: "Archive Relay",
							endpoint_url: "tcp://5.6.7.8:5678",
							role: "reader",
							clearance: "unclassified",
							last_seen_at: nowS - 3600,
							sync_enabled: false,
						},
					],
					local_node_id: "node_local",
				},
			});
		});

		await page.route("**/notifications/stream**", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "text/event-stream",
				body: "data: {}\n\n",
			});
		});
		await page.route("**/v1/memories**", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/v1/telemetry/errors", async (route) => {
			await route.fulfill({ json: { success: true } });
		});
		await page.route("**/v1/telemetry/**", async (route) => {
			await route.fulfill({ json: { success: true } });
		});

		await page.route("**/v1/chat/messages**", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/v1/chat/channels**", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/v1/telecom/**", async (route) => {
			await route.fulfill({ json: [] });
		});

		// Bypass onboarding and set authenticated session
		await page.addInitScript(() => {
			window.localStorage.setItem("xavier_onboarding_completed", "true");
			window.localStorage.setItem("xavier_token", "mock-jwt-token");
			window.sessionStorage.setItem("xavier.rt", "mock-rt-token");
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
	});

	test("loads TelecomHubView and renders PeerDiscoveryCard with actions", async ({
		page,
	}) => {
		const pageErrors: string[] = [];
		page.on("pageerror", (err) => pageErrors.push(String(err)));

		await page.goto("/#/telecom");
		await page.getByRole("tab", { name: /Network Peers/ }).click();

		const cards = page.getByTestId("peer-discovery-card");
		await expect(cards).toHaveCount(2);

		const gateway = cards.filter({ hasText: "Primary Gateway" });
		const archive = cards.filter({ hasText: "Archive Relay" });
		await expect(gateway.locator("h3")).toContainText("Primary Gateway");
		await expect(gateway.getByRole("button", { name: /Copy public key/i })).toBeVisible();
		await expect(gateway.getByRole("button", { name: /Start Direct Chat/i })).toBeVisible();

		// Status comes from last_seen_at (seconds), not from a simulated ping.
		await expect(gateway.getByTestId("latency-badge")).toContainText("Online");
		await expect(archive.getByTestId("latency-badge")).toContainText("Offline");
		await expect(gateway.getByTestId("latency-badge").locator("span").first()).toHaveClass(/bg-emerald/);

		// There is no ping endpoint in the daemon, so no Ping button is offered.
		await expect(page.getByRole("button", { name: /Ping node/i })).toHaveCount(0);

		const screenshotPath = path.join(
			ARTIFACT_DIR,
			"telecom_peer_discovery_e2e.png",
		);
		await page.screenshot({ path: screenshotPath, fullPage: true });
		expect(pageErrors).toEqual([]);
	});

	test("handles empty network peers gracefully", async ({ page }) => {
		const pageErrors: string[] = [];
		page.on("pageerror", (err) => pageErrors.push(String(err)));
		await page.route("**/v1/mesh/peers", async (route) => {
			await route.fulfill({
				json: {
					peers: [],
					local_node_id: "node_local",
				},
			});
		});

		await page.goto("/#/telecom");
		await page.getByRole("tab", { name: /Network Peers/ }).click();

		await expect(page.getByText("No peers discovered yet")).toBeVisible();
		await expect(page.getByTestId("peer-discovery-card")).toHaveCount(0);
		expect(pageErrors).toEqual([]);
	});
});
