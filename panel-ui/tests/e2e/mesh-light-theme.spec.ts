import { test, expect } from "@playwright/test";

test.describe("Mesh Light Theme", () => {
  test("renders correctly in light mode", async ({ page }, testInfo) => {
    await page.route("**/v1/mesh/peers", async (route) => {
      await route.fulfill({
        json: { peers: [], local_node_id: "local-node-1" }
      });
    });

    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "ok" } });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        json: { access_token: "mock_access", refresh_token: "mock_refresh" }
      });
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "light");
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
    });

    await page.goto("/#/mesh");
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    const html = page.locator("html");
    await expect(html).toHaveAttribute("data-theme", "studio-bone");

    const mainContainer = page.locator("main");
    await expect(mainContainer).toBeVisible();

    const isLight = await mainContainer.evaluate((el) => {
        const bg = window.getComputedStyle(el).backgroundColor;
        const match = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        if (!match) return false;
        const [_, r, g, b] = match;
        const rs = Number(r)/255, gs = Number(g)/255, bs = Number(b)/255;
        const rr = rs <= 0.03928 ? rs/12.92 : Math.pow((rs+0.055)/1.055, 2.4);
        const gg = gs <= 0.03928 ? gs/12.92 : Math.pow((gs+0.055)/1.055, 2.4);
        const bb = bs <= 0.03928 ? bs/12.92 : Math.pow((bs+0.055)/1.055, 2.4);
        const lum = 0.2126 * rr + 0.7152 * gg + 0.0722 * bb;
        return lum > 0.7;
    });
    expect(isLight).toBeTruthy();

    const header = page.locator("h1").first();
    const isDarkText = await header.evaluate((el) => {
        const color = window.getComputedStyle(el).color;
        const match = color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        if (!match) return false;
        const [_, r, g, b] = match;
        const rs = Number(r)/255, gs = Number(g)/255, bs = Number(b)/255;
        const rr = rs <= 0.03928 ? rs/12.92 : Math.pow((rs+0.055)/1.055, 2.4);
        const gg = gs <= 0.03928 ? gs/12.92 : Math.pow((gs+0.055)/1.055, 2.4);
        const bb = bs <= 0.03928 ? bs/12.92 : Math.pow((bs+0.055)/1.055, 2.4);
        const lum = 0.2126 * rr + 0.7152 * gg + 0.0722 * bb;
        return lum < 0.3;
    });
    expect(isDarkText).toBeTruthy();

    const createBtn = page.getByRole("button", { name: /Create Network/i }).first();
    if (await createBtn.isVisible()) {
        await createBtn.click();
        const wizard = page.locator("div.fixed.inset-0").first();
        await expect(wizard).toBeVisible();
        await page.screenshot({ path: testInfo.outputPath("mesh-light-wizard.png") });
    }

    await page.screenshot({ path: testInfo.outputPath("mesh-light.png") });

    // For dark mode, the theme stores in localStorage but playwright's reload needs the script to not overwrite it.
    // Instead of reload, just change localStorage and dispatch event if it listens, or navigate again.
    // Or let's just create a new page context. We can just use `vite-ui-theme="dark"` in localStorage.
  });
});

test.describe("Mesh Dark Theme", () => {
  test("renders correctly in dark mode", async ({ page }, testInfo) => {
    await page.route("**/v1/mesh/peers", async (route) => {
      await route.fulfill({
        json: { peers: [], local_node_id: "local-node-1" }
      });
    });

    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "ok" } });
    });

    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        json: { access_token: "mock_access", refresh_token: "mock_refresh" }
      });
    });

    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
      window.localStorage.setItem("vite-ui-theme", "dark");
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
    });

    await page.goto("/#/mesh");
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));

    const mainContainer = page.locator("main");
    await expect(mainContainer).toBeVisible();

    const isDark = await mainContainer.evaluate((el) => {
        const bg = window.getComputedStyle(el).backgroundColor;
        const match = bg.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        if (!match) return false;
        const [_, r, g, b] = match;
        const rs = Number(r)/255, gs = Number(g)/255, bs = Number(b)/255;
        const rr = rs <= 0.03928 ? rs/12.92 : Math.pow((rs+0.055)/1.055, 2.4);
        const gg = gs <= 0.03928 ? gs/12.92 : Math.pow((gs+0.055)/1.055, 2.4);
        const bb = bs <= 0.03928 ? bs/12.92 : Math.pow((bs+0.055)/1.055, 2.4);
        const lum = 0.2126 * rr + 0.7152 * gg + 0.0722 * bb;
        return lum < 0.3;
    });

    expect(isDark).toBeTruthy();
    expect(pageErrors).toEqual([]);
  });
});
