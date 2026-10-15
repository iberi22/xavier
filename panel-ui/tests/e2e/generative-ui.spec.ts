import { expect, type Page, test } from "@playwright/test";

const testToken = "mock-test-jwt-token";

interface MockSetupOptions {
	capturedAuthTokens: string[];
}

async function setupMockRoutes(page: Page, options: MockSetupOptions) {
	// Health probe
	await page.route("**/health", async (route) => {
		await route.fulfill({
			status: 200,
			contentType: "application/json",
			body: JSON.stringify({ status: "ok" }),
		});
	});

	// Provider config probe
	await page.route("**/v1/config/providers", async (route) => {
		await route.fulfill({
			status: 200,
			contentType: "application/json",
			body: JSON.stringify({
				providers: [
					{
						provider: "local",
						model: "mock-model",
						api_key: null,
						base_url: null,
					},
				],
			}),
		});
	});

	// Auth login endpoint
	await page.route("**/auth/login", async (route) => {
		await route.fulfill({
			status: 200,
			contentType: "application/json",
			body: JSON.stringify({
				user: {
					id: "op-1",
					email: "operator@xavier.local",
					role: "admin",
				},
				access_token: testToken,
				refresh_token: "mock-refresh-token",
			}),
		});
	});

	// Protected panel APIs
	let activeThreadId: string | null = null;
	const messagesList: Array<{
		id: string;
		role: string;
		plain_text: string;
		openui_lang?: string | null;
		created_at: string;
	}> = [];

	await page.route("**/panel/api/**", async (route) => {
		const headers = route.request().headers();
		const authToken = headers["x-xavier-token"];
		if (authToken) {
			options.capturedAuthTokens.push(authToken);
		}

		const url = new URL(route.request().url());
		const pathname = url.pathname.replace(/\/+$/, "") || "/";
		const method = route.request().method();

		if (pathname === "/panel/api/bookmarks") {
			return route.fulfill({ json: [] });
		}
		if (pathname === "/panel/api/widgets") {
			return route.fulfill({ json: [] });
		}
		if (pathname === "/panel/api/graph") {
			return route.fulfill({ json: { data: { nodes: [], links: [] } } });
		}

		const threadDetailMatch = pathname.match(
			/^\/panel\/api\/threads\/([^/]+)$/,
		);
		if (method === "GET" && threadDetailMatch) {
			const threadId = threadDetailMatch[1];
			return route.fulfill({
				json: {
					thread: {
						id: threadId,
						title: "Generative UI Session",
						created_at: new Date().toISOString(),
						updated_at: new Date().toISOString(),
						last_preview: "",
						message_count: messagesList.length,
					},
					messages: [...messagesList],
				},
			});
		}

		if (pathname === "/panel/api/threads") {
			if (method === "POST") {
				activeThreadId = "thread-gen-1";
				return route.fulfill({
					json: {
						id: activeThreadId,
						title: "Generative UI Session",
						created_at: new Date().toISOString(),
						updated_at: new Date().toISOString(),
						last_preview: "",
						message_count: 0,
					},
				});
			}
			if (method === "GET") {
				return route.fulfill({ json: [] });
			}
		}

		if (pathname === "/panel/api/chat" && method === "POST") {
			const postData = route.request().postDataJSON() as {
				message?: string;
				thread_id?: string;
			};
			const userPrompt = postData?.message ?? "";
			const userMsgId = `msg-u-${Date.now()}`;
			const assistantMsgId = `msg-a-${Date.now()}`;

			messagesList.push({
				id: userMsgId,
				role: "user",
				plain_text: userPrompt,
				created_at: new Date().toISOString(),
			});

			if (userPrompt.includes("malformed")) {
				messagesList.push({
					id: assistantMsgId,
					role: "assistant",
					plain_text: "Fallback plain text: malformed markup cannot be parsed.",
					openui_lang: "<SectionBlock><Card>unclosed tag without ending",
					created_at: new Date().toISOString(),
				});
			} else {
				messagesList.push({
					id: assistantMsgId,
					role: "assistant",
					plain_text:
						"Here is the structured breakdown of Xavier cognitive memory architecture.",
					openui_lang:
						'<SectionBlock title="Cognitive Core" description="Vector memory engine"><Card title="Vector Store" description="Persistent sqlite-vec engine"><TextContent content="High-dimensional embeddings with cosine distance." /><ListBlock items=\'[{"title":"BM25 Hybrid","description":"Keyword ranking","meta":"Fast"},{"title":"Vector Retrieval","description":"Cosine similarity","meta":"Dense"}]\' /></Card></SectionBlock>',
					created_at: new Date().toISOString(),
				});
			}

			return route.fulfill({
				json: {
					thread: {
						id: activeThreadId ?? "thread-gen-1",
						title: "Generative UI Session",
						created_at: new Date().toISOString(),
						updated_at: new Date().toISOString(),
						last_preview: "",
						message_count: messagesList.length,
					},
					messages: [...messagesList],
				},
			});
		}

		return route.fulfill({ status: 404 });
	});
}

test.describe("Xavier Generative UI (OpenUI)", () => {
	test("logs in, sends prompt, renders structured SectionBlock/Card/ListBlock and falls back on malformed markup", async ({
		page,
	}) => {
		const capturedAuthTokens: string[] = [];
		await setupMockRoutes(page, { capturedAuthTokens });

		// Mark onboarding completed so the app lands on the login page directly
		await page.addInitScript(() => {
			window.localStorage.setItem("xavier_onboarding_completed", "true");
		});

		await page.goto("/");

		// 1. Login flow as the app executes it
		await expect(
			page.getByRole("heading", { name: "XAVIER LOGIN" }),
		).toBeVisible();
		await expect(
			page.getByText("Connect to the secure cognitive core"),
		).toBeVisible();

		await page
			.getByPlaceholder("operator@xavier.local")
			.fill("operator@xavier.local");
		await page.getByLabel("Password", { exact: true }).fill("password123");
		await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();

		// 2. Command input in authenticated panel shell
		const composer = page.getByTestId("command-input");
		await expect(composer).toBeVisible();

		// 3. Send structured prompt
		const prompt =
			"Explain xavier memory and show the answer as a structured UI.";
		await composer.fill(prompt);
		const sendButton = page.getByRole("button", { name: "Send command" });
		await expect(sendButton).toBeEnabled();
		await sendButton.click();

		// 4. Assert OpenUI render surface and structured card components appear
		const renderSurface = page.getByTestId("openui-render-surface");
		await expect(renderSurface).toBeVisible();
		await expect(
			renderSurface.getByText("OpenUI Render Surface"),
		).toBeVisible();

		const sectionBlock = page.getByTestId("openui-section-block");
		await expect(sectionBlock).toBeVisible();
		await expect(sectionBlock.getByText("Cognitive Core")).toBeVisible();
		await expect(sectionBlock.getByText("Vector memory engine")).toBeVisible();

		const card = page.getByTestId("openui-card");
		await expect(card).toBeVisible();
		await expect(card.getByText("Vector Store")).toBeVisible();
		await expect(card.getByText("Persistent sqlite-vec engine")).toBeVisible();
		await expect(
			card.getByText("High-dimensional embeddings with cosine distance."),
		).toBeVisible();

		const listBlock = page.getByTestId("openui-list-block");
		await expect(listBlock).toBeVisible();
		await expect(listBlock.getByText("BM25 Hybrid")).toBeVisible();
		await expect(listBlock.getByText("Vector Retrieval")).toBeVisible();
		await expect(listBlock.getByText("Fast")).toBeVisible();
		await expect(listBlock.getByText("Dense")).toBeVisible();

		// Assert assistant plain text is also displayed below the render surface
		await expect(page.getByTestId("assistant-plain-text")).toContainText(
			"Here is the structured breakdown of Xavier cognitive memory architecture.",
		);

		// 5. Send prompt triggering malformed markup and assert plain-text fallback
		const malformedPrompt = "Trigger malformed response";
		await composer.fill(malformedPrompt);
		await sendButton.click();

		// Target that specific message (within the last assistant message) rather than using .first()
		const lastAssistantMessage = page.getByTestId("assistant-card").last();
		await expect(
			lastAssistantMessage.getByText(
				"Fallback plain text: malformed markup cannot be parsed.",
			),
		).toBeVisible();
		await expect(
			lastAssistantMessage.getByTestId("plain-text-fallback"),
		).toBeVisible();
		await expect(
			lastAssistantMessage.getByTestId("openui-render-surface"),
		).toHaveCount(0);

		// Assert that the malformed markup did not create another render surface
		await expect(page.getByTestId("openui-render-surface")).toHaveCount(1);

		// 6. Assert protected API calls carry the auth header
		expect(capturedAuthTokens.length).toBeGreaterThan(0);
		for (const token of capturedAuthTokens) {
			expect(token).toBe(testToken);
		}
	});
});
