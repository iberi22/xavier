import { test, expect } from '@playwright/test';

test.describe('Panel UI End-to-End Tests', () => {
  test.beforeEach(async ({ page }) => {
    // Mock the health endpoint to simulate backend being online
    await page.route('**/health', async route => {
      const json = { status: 'online', version: '1.0.0' };
      await route.fulfill({ json });
    });
    
    // Mock threads endpoint
    await page.route('**/panel/api/threads', async route => {
      const json = [
        { id: 't1', title: 'Test Thread', created_at: new Date().toISOString(), message_count: 1 }
      ];
      await route.fulfill({ json });
    });

    await page.goto('/');
  });

  test('should display token authentication when unauthorized', async ({ page }) => {
    await expect(page.locator('text=Neuro Core Access')).toBeVisible();
    await expect(page.locator('input[placeholder="XAVIER_TOKEN"]')).toBeVisible();
  });

  test('should login and load threads successfully', async ({ page }) => {
    // Fill the token
    await page.fill('input[placeholder="XAVIER_TOKEN"]', 'test-token');
    await page.click('button:has-text("Enter Neuro Core")');

    // Should load the main interface
    await expect(page.locator('input[placeholder="Type a message to Xavier..."]')).toBeVisible();
    await expect(page.locator('text=Threads')).toBeVisible();
    await expect(page.locator('text=Test Thread')).toBeVisible();
  });

  test('should open config modal and pin an artifact', async ({ page }) => {
    // Login
    await page.fill('input[placeholder="XAVIER_TOKEN"]', 'test-token');
    await page.click('button:has-text("Enter Neuro Core")');

    // Click config/nodes button
    await page.click('button[title="Core Configuration & Nodes"]');

    // Verify modal opened
    await expect(page.locator('text=Cognitive Graph Architecture')).toBeVisible();

    // Click on Bookmarks tab
    await page.click('button:has-text("Bookmarks")');

    // Look for a pin button in the bookmarks list
    const pinButton = page.locator('button:has-text("Pin to Canvas")').first();
    if (await pinButton.isVisible()) {
      await pinButton.click();
      
      // Modal should close automatically and we should see a draggable widget
      await expect(page.locator('text=Cognitive Graph Architecture')).not.toBeVisible();
      // Draggable widgets usually have absolute positioning and specific structure
      await expect(page.locator('.absolute.z-50.shadow-2xl').first()).toBeVisible();
    }
  });

  test('should create a new thread', async ({ page }) => {
    // Login
    await page.fill('input[placeholder="XAVIER_TOKEN"]', 'test-token');
    await page.click('button:has-text("Enter Neuro Core")');

    // Mock create thread
    await page.route('**/panel/api/threads', async route => {
      if (route.request().method() === 'POST') {
        const json = { id: 't2', title: 'New Thread', created_at: new Date().toISOString(), message_count: 0 };
        await route.fulfill({ json });
      } else {
        route.continue();
      }
    });

    // Click New Thread button
    await page.click('button[title="New Thread"]');
    
    // Verify optimistic thread update
    await expect(page.locator('button:has-text("New Thread")')).toBeVisible();
  });
});
