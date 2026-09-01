import { expect, test } from "@playwright/test";

test.describe("InputArea browser fallback E2E Tests", () => {
  test("should display FolderPlus button and handle browser file fallback without crashing", async ({
    page,
  }) => {
    // Navigate to application
    await page.goto("/");

    // Authenticate if needed
    const tokenInput = page.locator('textarea[placeholder="XAVIER_TOKEN"]');
    if (await tokenInput.isVisible()) {
      await tokenInput.fill("test-token");
      await page.click('button:has-text("Enter panel")');
    }

    // Verify FolderPlus button is visible
    const folderButton = page.locator('button[aria-label="Add project codebase"]');
    await expect(folderButton).toBeVisible();

    // Ensure hidden file input exists
    const hiddenFileInput = page.locator('input[type="file"]');
    await expect(hiddenFileInput).toBeAttached();

    // Click FolderPlus button in browser mode (triggers file input click without throwing error)
    await folderButton.click();

    // Simulate file input change event
    await hiddenFileInput.setInputFiles([
      {
        name: "index.ts",
        mimeType: "text/plain",
        buffer: Buffer.from("console.log('hello');"),
      },
    ]);

    // Verify that system message appears in system chat / messages
    await expect(page.locator("text=Carpeta seleccionada:")).toBeVisible();
  });
});
