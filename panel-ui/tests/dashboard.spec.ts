import { expect, test } from "@playwright/test";

test.describe("Dashboard and UI Islands", () => {
  test.beforeEach(async ({ page }) => {
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    await page.route("**/panel/api/threads", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/panel/api/bookmarks", async (route) => {
      await route.fulfill({
        json: [
          {
            id: "b1",
            title: "Test Artifact",
            type: "Data Card",
            category: "Testing",
            date: "2026-05-10",
            metadata: {},
          },
        ],
      });
    });

    await page.route("**/panel/api/widgets", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/panel/api/graph", async (route) => {
      await route.fulfill({
        json: {
          data: {
            nodes: [
              {
                id: "n1",
                label: "Node 1",
                type: "project",
                description: "Test Node Description",
              },
            ],
            links: [],
          },
        },
      });
    });

    await page.goto("/");
    await page.evaluate(() => {
      localStorage.setItem("xavier_onboarding_completed", "true");
    });
    await page.fill('input[placeholder="XAVIER_TOKEN"]', "test-token");
    await page.click('button:has-text("INITIALIZE SESSION")');
  });

  test("should toggle modules in TopStatusBar", async ({ page }) => {
    // Identity pill is always visible
    await expect(page.getByText("Xavier Beta")).toBeVisible();

    // Hover near identity to show gear
    await page.locator('div:has-text("Xavier Beta")').hover();
    const gear = page.locator('button[data-title="Configure Status Bar"]');
    await expect(gear).toBeVisible();
    await gear.click();

    // Verify popover
    await expect(page.getByText("Modules")).toBeVisible();

    // Toggle System Resources (it is enabled by default)
    const resourcesPill = page.locator(
      'div[data-title="Average CPU Usage: 14%"]',
    );
    await expect(resourcesPill).toBeVisible();

    await page.click('button:has-text("System Resources")');
    await expect(resourcesPill).toBeHidden();

    await page.click('button:has-text("System Resources")');
    await expect(resourcesPill).toBeVisible();
  });

  test("should navigate ConfigModal and interact with Knowledge Graph stats", async ({
    page,
  }) => {
    // Open Config Modal
    await page.click('button[title="Open Control Node"]');
    await expect(page.getByText("Knowledge Graph")).toBeVisible();

    // Switch to Knowledge Graph tab
    await page.click('button:has-text("Knowledge Graph")');

    // Verify GraphView is rendered (canvas should be present)
    await expect(page.locator("canvas")).toBeVisible();

    // Verify and interaction with System Diagnostics (stats) in GraphView
    const statsTrigger = page.getByText("System Diagnostics");
    await expect(statsTrigger).toBeVisible();
    await statsTrigger.click();

    await expect(page.getByText("Total Nodes:")).toBeVisible();
    await expect(page.getByText("↳ Projects:")).toBeVisible();

    // Switch to Configuration tab
    await page.click('button:has-text("Configuration")');
    await expect(page.getByText("Topology Stats")).toBeVisible();

    // Check Configuration sub-tabs
    await page.click('button:has-text("Memory & Layers")');
    await expect(page.getByText("Memory Management")).toBeVisible();
    await expect(page.getByText("Backend Engine")).toBeVisible();

    await page.click('button:has-text("Advanced & Security")');
    await expect(page.getByText("Advanced System Settings")).toBeVisible();
    await expect(page.getByText("Token Secret")).toBeVisible();
  });

  test("should pin a widget from Saved Artifacts to canvas", async ({
    page,
  }) => {
    await page.click('button[title="Open Control Node"]');

    // Switch to Saved Artifacts
    await page.click('button:has-text("Saved Artifacts")');
    await expect(page.getByText("Test Artifact")).toBeVisible();

    // Pin to Canvas
    await page.click('button[title="Pin to Canvas"]');

    // Modal should NOT automatically close based on App.tsx:
    // handlePinArtifact sets widgets and setIsConfigOpen(false).
    await expect(page.getByText("Saved Artifacts")).toBeHidden();

    // Verify widget on canvas
    const widget = page.locator(".absolute.z-50"); // DraggableWidget class-ish (shadow-2xl was seen in App.tsx mock/spec)
    // Wait, App.tsx uses <DraggableWidget />
    // DraggableWidget.tsx uses: className="absolute z-50 ... shadow-2xl pointer-events-auto"
    await expect(page.getByText("Test Artifact")).toBeVisible();

    // Test removal
    await page.locator('button[title="Remove widget"]').click();
    await expect(page.getByText("Test Artifact")).toBeHidden();
  });
});
