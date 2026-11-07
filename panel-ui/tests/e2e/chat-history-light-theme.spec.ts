import { expect, test } from "@playwright/test";

test.describe("ChatHistory Light Theme Verification", () => {
  test.beforeEach(async ({ page }) => {
    // Mock health
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    // Mock initial threads
    await page.route("**/panel/api/threads", async (route) => {
      if (route.request().method() === "POST") {
        await route.fulfill({
          json: {
            id: "t1",
            title: "Test Thread",
            created_at: new Date().toISOString(),
            message_count: 0,
          },
        });
      } else {
        await route.fulfill({ json: [] });
      }
    });

    await page.route("**/v1/config/providers", async (route) => {
      await route.fulfill({
        json: { providers: [{ provider: "local", model: "test-model", api_key: null, base_url: null }] },
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

    await page.route("**/panel/api/bookmarks", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/panel/api/widgets", async (route) => {
      await route.fulfill({ json: [] });
    });
    await page.route("**/panel/api/graph", async (route) => {
      await route.fulfill({ json: { data: { nodes: [], links: [] } } });
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-jwt-token");
      window.sessionStorage.setItem("xavier.rt", "mock-rt-token");
      window.localStorage.setItem("xavier-theme-settings", JSON.stringify({ mode: "studio-bone" }));
      window.localStorage.setItem("xavier-theme", "studio-bone");
      window.localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            token: "mock-jwt-token",
            isAuthenticated: true,
            user: { id: "1", email: "operator@xavier.local", role: "admin" },
          },
          version: 0,
        })
      );
      document.documentElement.setAttribute('data-theme', 'studio-bone');
      document.documentElement.classList.add('studio-bone');
      document.documentElement.classList.remove('studio-dark');
      document.documentElement.classList.remove('dark');
    });

    await page.goto("/");

    // Instead of fighting the login UI which might be flaking between magic link / password
    // we assume the init script bypasses it, but we wait for the command input explicitly.
    // If it gets stuck on login, we try to detect and pass it
    const loginButton = page.locator('button:has-text("INITIALIZE SESSION")');
    if (await loginButton.isVisible({ timeout: 1000 }).catch(()=>false)) {
      const otherMethodsBtn = page.getByText("Otros métodos de acceso");
      if (await otherMethodsBtn.isVisible().catch(()=>false)) {
        await otherMethodsBtn.click();
      }
      await page.fill('input[type="email"]', "operator@xavier.local");
      const pwd = page.locator('input[type="password"]');
      if (await pwd.isVisible()) await pwd.fill("password123");
      await loginButton.first().click();
    }
  });

  test("should render chat history with correct colors in studio-bone theme", async ({ page }) => {
    // Wait for the command input to ensure the app is ready
    const input = page.locator('[data-testid="command-input"], input[placeholder*="Ask anything"], input[placeholder="Initialize command sequence..."]').first();
    await expect(input).toBeVisible({ timeout: 15000 });

    // We send the chat
    await page.route("**/panel/api/chat", async (route) => {
      await route.fulfill({
        json: {
          thread: { id: "t1", title: "Theme test", message_count: 2 },
          messages: [
            {
              id: "1",
              role: "user",
              plain_text: "Test message",
              created_at: new Date().toISOString(),
            },
            {
              id: "2",
              role: "assistant",
              plain_text: "Response in light theme",
              created_at: new Date().toISOString(),
            },
          ],
        },
      });
    });

    await input.fill("Test message");

    const sendButton = page.locator('button[title="Send command"]');
    await expect(sendButton).toBeEnabled();
    await sendButton.click();

    // Wait for messages to appear
    await expect(page.getByText("Test message").first()).toBeVisible();
    await expect(page.getByText("Response in light theme")).toBeVisible();

    // Verify User Message styling
    const userCard = page.getByTestId("user-card").first();
    const userBubble = userCard.locator(".rounded-2xl").first();

    // Check that we're using theme-aware colors
    await expect(userBubble).toHaveClass(/bg-black\/\[0\.05\]/);
    await expect(userBubble).toHaveClass(/dark:bg-white\/\[0\.05\]/);
    await expect(userBubble).toHaveClass(/text-slate-800/);
    await expect(userBubble).toHaveClass(/dark:text-white\/80/);

    // Assistant message check
    const assistantCard = page.getByTestId("assistant-card").first();
    const assistantBubble = assistantCard.locator(".rounded-2xl").first();

    // Test the luminance by expecting dark/light mode classes
    await expect(assistantBubble).toHaveClass(/bg-emerald-500\/\[0\.05\]/);
    await expect(assistantBubble).toHaveClass(/dark:bg-\[#39ff14\]\/\[0\.025\]/);
  });
});
