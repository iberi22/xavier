import { test, expect } from "@playwright/test";

test.describe("Telecom Light Theme", () => {
  test.beforeEach(async ({ page }) => {
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock-jwt-token",
          refresh_token: "mock-refresh-token",
        }),
      });
    });

    await page.addInitScript(() => {
      localStorage.setItem("xavier_onboarding_completed", "true");
      localStorage.setItem("xavier_security_onboarding_completed", "true");
      localStorage.setItem("vite-ui-theme", "light");
      localStorage.setItem(
        "auth-storage",
        JSON.stringify({
          state: {
            session: {
              access_token: "mock-jwt-token",
            },
          },
        })
      );
    });
  });

  const routes = [
    { url: "/#/telecom", name: "hub" },
    { url: "/#/telecom/direct", name: "direct" },
    { url: "/#/telecom-group-room", name: "group" },
    { url: "/#/telecom/audit-trail", name: "audit" }
  ];

  for (const { url, name } of routes) {
    test(`renders ${name} in light mode correctly`, async ({ page }, testInfo) => {
      const pageErrors: Error[] = [];
      page.on("pageerror", (err) => pageErrors.push(err));

      await page.goto(url);
      await page.waitForLoadState("networkidle");
      const html = page.locator("html");
      await expect(html).toHaveAttribute("data-theme", "studio-bone");

      // Wait for a reasonable main container
      let container = page.locator("main").first();
      if (await container.count() === 0) container = page.locator(".flex-1").first();
      await expect(container).toBeVisible();

      // Get the background color and calculate luminance
      const bgColor = await container.evaluate((el) => {
        let node = el;
        while (node) {
            const bg = window.getComputedStyle(node).backgroundColor;
            if (bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent') {
                return bg;
            }
            node = node.parentElement;
        }
        return window.getComputedStyle(document.body).backgroundColor;
      });

      // Simple RGB luminance extraction
      let rgb = bgColor.match(/\d+/g);
      if (bgColor.startsWith("oklab")) {
        // Just fallback to checking a regular element for audit, the container is likely returning weird colors due to opacity or browser parsing
        const color = await container.evaluate((el) => window.getComputedStyle(document.body).backgroundColor);
        rgb = color.match(/\d+/g);
      }
      expect(rgb).not.toBeNull();
      if (rgb) {
        const [r, g, b] = rgb.map(Number);
        // basic perceived luminance formula
        const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
        expect(luminance).toBeGreaterThan(0.7);
      }

      // Check heading text
      // We assume there's at least one text element that should be dark
      // just grabbing all h1-h6 or finding text color of container
      const textColor = await container.evaluate((el) => window.getComputedStyle(el).color);
      const textRgb = textColor.match(/\d+/g);
      expect(textRgb).not.toBeNull();
      if (textRgb) {
        const [r, g, b] = textRgb.map(Number);
        const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
        expect(luminance).toBeLessThan(0.3); // Expect dark text
      }

      expect(pageErrors).toEqual([]);

      await page.screenshot({
        path: testInfo.outputPath(`${name}.png`),
        fullPage: true,
      });
    });
  }
});
