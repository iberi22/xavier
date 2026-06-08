import { test, expect } from '@playwright/test';

test.describe('Agent Chat Interactions', () => {
  test.beforeEach(async ({ page }) => {
    // Mock health
    await page.route('**/health', async route => {
      await route.fulfill({ json: { status: 'online' } });
    });

    // Mock initial threads
    await page.route('**/panel/api/threads', async route => {
      if (route.request().method() === 'GET') {
        await route.fulfill({ json: [] });
      } else {
        route.continue();
      }
    });

    // Mock initial state
    await page.route('**/panel/api/bookmarks', async route => {
      await route.fulfill({ json: [] });
    });
    await page.route('**/panel/api/widgets', async route => {
      await route.fulfill({ json: [] });
    });
    await page.route('**/panel/api/graph', async route => {
      await route.fulfill({ json: { data: { nodes: [], links: [] } } });
    });

    await page.goto('/');

    // Bypass auth
    await page.evaluate(() => {
      localStorage.setItem('xavier_onboarding_completed', 'true');
    });
    await page.fill('input[placeholder="XAVIER_TOKEN"]', 'test-token');
    await page.click('button:has-text("INITIALIZE SESSION")');

    await expect(page.locator('input[placeholder="Initialize command sequence..."]')).toBeVisible();
  });

  test('should send a message and show assistant response', async ({ page }) => {
    const userInput = 'Hello Xavier';

    // Mock thread creation and chat response
    await page.route('**/panel/api/threads', async route => {
      if (route.request().method() === 'POST') {
        await route.fulfill({ json: { id: 't1', title: 'Hello Xavier', created_at: new Date().toISOString(), message_count: 0 } });
      }
    });

    await page.route('**/panel/api/chat', async route => {
      await route.fulfill({
        json: {
          thread: { id: 't1', title: 'Hello Xavier', message_count: 2 },
          messages: [
            { id: '1', role: 'user', plain_text: userInput, created_at: new Date().toISOString() },
            { id: '2', role: 'assistant', plain_text: 'Hello! I am Xavier, your cognitive assistant.', created_at: new Date().toISOString() }
          ]
        }
      });
    });

    const input = page.locator('input[placeholder="Initialize command sequence..."]');
    await input.fill(userInput);

    // Verify send button is enabled
    const sendButton = page.locator('button[title="Send command"]');
    await expect(sendButton).toBeEnabled();

    await sendButton.click();

    // Verify input cleared
    await expect(input).toHaveValue('');

    // Verify messages in ChatHistory
    await expect(page.getByText(userInput)).toBeVisible();
    await expect(page.getByText('Hello! I am Xavier, your cognitive assistant.')).toBeVisible();
    await expect(page.getByText('Xavier Agent')).toBeVisible();
  });

  test('should handle command triggers starting with /', async ({ page }) => {
    const command = '/scan .';

    let chatPayload: any = null;
    await page.route('**/panel/api/chat', async route => {
      chatPayload = route.request().postDataJSON();
      await route.fulfill({
        json: {
          thread: { id: 't1', title: 'Command', message_count: 2 },
          messages: [
            { id: '1', role: 'user', plain_text: command, created_at: new Date().toISOString() },
            { id: '2', role: 'assistant', plain_text: 'Initiating scan of current directory...', created_at: new Date().toISOString() }
          ]
        }
      });
    });

    await page.fill('input[placeholder="Initialize command sequence..."]', command);
    await page.click('button[title="Send command"]');

    await expect(page.getByText('Initiating scan of current directory...')).toBeVisible();
    expect(chatPayload.message).toBe(command);
  });

  test('should handle backend errors during chat', async ({ page }) => {
    await page.route('**/panel/api/chat', async route => {
      await route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Internal Server Error' })
      });
    });

    await page.fill('input[placeholder="Initialize command sequence..."]', 'Trigger error');
    await page.click('button[title="Send command"]');

    // The user message should still appear (optimistic UI)
    await expect(page.getByText('Trigger error')).toBeVisible();

    // Since App.tsx sets error state but doesn't render it yet,
    // we just verify it doesn't crash and maybe check console if we wanted.
    // For 100% E2E we verify the UI state.
    await expect(page.getByText('Xavier Agent')).not.toBeVisible();
  });
});
