import { test, expect } from '@playwright/test';

test.describe('Panel UI End-to-End Tests', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('should display token authentication when unauthorized', async ({ page }) => {
    await expect(page.locator('text=Xavier Internal Panel')).toBeVisible();
    await expect(page.locator('textarea[placeholder="XAVIER_TOKEN"]')).toBeVisible();
  });

  test('should login and load threads successfully', async ({ page }) => {
    // Fill the token
    await page.fill('textarea[placeholder="XAVIER_TOKEN"]', 'test-token');
    await page.click('button:has-text("Enter panel")');

    // Should load the main interface
    await expect(page.locator('textarea[placeholder="Ask Xavier for memory, code, or a structured answer..."]')).toBeVisible();
    await expect(page.locator('text=Render Agent Console')).toBeVisible();
  });

  test('should verify persisted artifacts in sidebar', async ({ page }) => {
    // Login
    await page.fill('textarea[placeholder="XAVIER_TOKEN"]', 'test-token');
    await page.click('button:has-text("Enter panel")');

    // Verify sidebar artifacts section
    await expect(page.locator('text=Persisted Artifacts')).toBeVisible();
    await expect(page.locator('text=Bookmarks:')).toBeVisible();
    await expect(page.locator('text=Widgets:')).toBeVisible();
    await expect(page.locator('text=Graph:')).toBeVisible();
  });

  test('should create a new thread', async ({ page }) => {
    // Login
    await page.fill('textarea[placeholder="XAVIER_TOKEN"]', 'test-token');
    await page.click('button:has-text("Enter panel")');

    // Click New thread button
    await page.click('button:has-text("New thread")');
    
    // Verify thread appears in the list
    await expect(page.locator('.thread-item').first()).toBeVisible();
    await expect(page.locator('.thread-item').first().locator('strong')).toHaveText('New Thread');
  });
});
