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

test.describe("Telecom Voice Notes Player E2E Verification", () => {
	test.beforeEach(async ({ page }) => {
		await page.route("**/health", async (route) => {
			await route.fulfill({ json: { status: "online" } });
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

		await page.route("**/panel/api/bookmarks", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/panel/api/widgets", async (route) => {
			await route.fulfill({ json: [] });
		});
		await page.route("**/panel/api/graph", async (route) => {
			await route.fulfill({ json: { data: { nodes: [], links: [] } } });
		});
		await page.route("**/panel/api/threads", async (route) => {
			await route.fulfill({ json: [] });
		});

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
				}),
			);

			window.localStorage.setItem(
				"xavier_telecom_rooms",
				JSON.stringify([
					{
						id: "room_1",
						name: "Alpha Team",
						category: "groups",
						isOnline: true,
						unreadCount: 1,
						lastMessage: "Voice note received",
						isEncrypted: true,
						timestamp: new Date().toISOString(),
					},
				]),
			);

			window.localStorage.setItem(
				"xavier_telecom_messages",
				JSON.stringify({
					room_1: [
						{
							id: "msg_voice_1",
							roomId: "room_1",
							senderAlias: "Agent K",
							senderNodeId: "node-k-123",
							content: "Please check this voice report.",
							timestamp: new Date().toISOString(),
							encrypted: true,
							isSelf: false,
							attachment: {
								name: "report_audio_01.opus",
								size: "245 KB",
								type: "audio/opus",
							},
						},
					],
				}),
			);
		});

		await page.goto("/#/mesh");
	});

	test("renders VoiceNotePlayer in chat message, plays audio, and captures waveform visually", async ({
		page,
	}) => {
		await page.waitForLoadState("domcontentloaded");

		const pageScreenshotPath = path.join(
			ARTIFACT_DIR,
			"telecom_voice_notes_full.png",
		);

		const groupsTab = page.locator(
			'button[role="tab"]:has-text("Group Rooms")',
		);

		await groupsTab.waitFor({ state: "visible", timeout: 10000 });
		await groupsTab.click();

		const alphaRoom = page.locator('div[role="button"]:has-text("Alpha Team")');
		await alphaRoom.waitFor({ state: "visible", timeout: 10000 });
		await alphaRoom.click();

		const playButton = page.locator('button[aria-label="Play voice note"]');
		await expect(playButton).toBeVisible({ timeout: 10000 });

		const slider = page.locator(
			'canvas[role="slider"][aria-label="Voice note playback progress"]',
		);
		await expect(slider).toBeVisible();

		// Duration label
		const durationLabel = page.locator("text=/\\d{2}:\\d{2}/").first();
		await expect(durationLabel).toBeVisible();

		// Click play button
		await playButton.click();

		// Assert playing state transition
		const pauseButton = page.locator('button[aria-label="Pause voice note"]');
		await expect(pauseButton).toBeVisible();

		const playerScreenshotPath = path.join(
			ARTIFACT_DIR,
			"voice_note_player_playing.png",
		);
		const voiceNotePlayerArea = page
			.locator('div:has(> canvas[role="slider"])')
			.first();
		await voiceNotePlayerArea.screenshot({ path: playerScreenshotPath });

		await page.screenshot({ path: pageScreenshotPath, fullPage: true });
	});
});
