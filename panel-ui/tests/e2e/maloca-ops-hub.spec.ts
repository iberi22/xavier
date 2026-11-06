import { test, expect, type Page } from "@playwright/test";

// Shapes mirror panel-ui/src/maloca/api.ts (MalocaPack, MarketListing, MarketReceipt,
// MarketConsentRecord), which follow the daemon's /maloca/* and /v2/marketplace/* routes.
const PACK = {
  generated_at: "2026-11-06T18:00:00Z",
  codegraph_indexed_at: "2026-11-06T17:30:00Z",
  codegraph_head: "cb8a4f3b",
  features_total: 42,
  features_draft: 7,
  gaps_zero_symbol_modules: ["src/legacy/bridge.rs", "src/maloca/orphan.rs"],
  decisions_count: 13,
  support_open: 3,
  inbox_open: 5,
};

const NOW_MS = Date.UTC(2026, 10, 6, 18, 0, 0);
const LISTINGS = [
  {
    id: "lst-biology-qa",
    seller_id: "node-seller-0a1b2c3d",
    title: "Biology QA Dataset",
    description: "Curated question/answer pairs with provenance.",
    price_credits: 25,
    license: "cc-by-4.0",
    terms_version: "v1.0",
    created_at_ms: NOW_MS - 3_600_000,
    revoked_at_ms: null,
  },
  {
    id: "lst-old-pack",
    seller_id: "node-seller-9f8e7d6c",
    title: "Deprecated Telemetry Pack",
    description: "Withdrawn by its seller.",
    price_credits: 5,
    license: "cc-by-4.0",
    terms_version: "v1.0",
    created_at_ms: NOW_MS - 86_400_000,
    revoked_at_ms: NOW_MS - 1_000,
  },
];

async function mockMarketplace(page: Page) {
  const receipts: Array<Record<string, unknown>> = [];
  const purchases: Array<Record<string, unknown>> = [];
  await page.route("**/v2/marketplace/listings", (route) => route.fulfill({ json: LISTINGS }));
  await page.route("**/v2/marketplace/consent?**", (route) =>
    route.fulfill({
      json: {
        node_id: "local-node",
        state: "local",
        terms_version: "v1.0",
        consent_version: "v1.0",
        updated_at_ms: NOW_MS,
      },
    }),
  );
  await page.route("**/v2/marketplace/receipts**", async (route) => {
    const req = route.request();
    if (req.method() === "POST") {
      const body = req.postDataJSON() as { listing_id: string; buyer_id: string };
      purchases.push(body);
      const receipt = {
        id: `rcpt-${purchases.length}`,
        listing_id: body.listing_id,
        buyer_id: body.buyer_id,
        amount_credits: 25,
        currency: "credits",
        created_at_ms: NOW_MS,
      };
      receipts.push(receipt);
      await route.fulfill({ json: receipt });
      return;
    }
    await route.fulfill({ json: receipts });
  });
  return { purchases };
}

test.describe("Maloca Ops Hub (#/maloca)", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept API routes
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online" } });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
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

    // AuthProvider calls authClient.refresh() on every mount (panel-ui/src/auth/AuthProvider.tsx);
    // this test navigates straight to a gated route (#/maloca) with no explicit login
    // click, so isAuthenticated only ever becomes true via this call. Shape matches the real
    // backend (refresh_handler's AuthResponse, src/auth2/mod.rs): { access_token, refresh_token }.
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock.jwt.access-token-maloca",
          refresh_token: "mock-refresh-token-maloca",
        }),
      });
    });

    // Bypass onboarding and set authenticated session
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_security_onboarding_completed", "true");
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
        })
      );
    });
  });

  test("overview renders the daemon pack on first load", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));
    await page.route("**/maloca/pack", (route) => route.fulfill({ json: PACK }));

    await page.goto("/#/maloca");

    await expect(page.getByRole("heading", { name: "Maloca Ops Hub" })).toBeVisible();
    await expect(page.getByRole("button", { name: /Hub Overview/ })).toHaveAttribute("aria-current", "page");
    await expect(page.getByText(/^42\s*TOTAL$/)).toBeVisible();
    await expect(page.getByText(/^7\s*in draft state$/)).toBeVisible();
    await expect(page.getByText(/^3\s*OPEN$/)).toBeVisible();
    await expect(page.getByText(/^13\s*RECORDED$/)).toBeVisible();
    await expect(page.getByText(/^5\s*PENDING$/)).toBeVisible();
    await expect(page.getByText("cb8a4f3b")).toBeVisible();
    await expect(page.getByText("src/legacy/bridge.rs")).toBeVisible();
    await expect(page.getByText("2 modules")).toBeVisible();
    await expect(page.getByText(/System Error/)).toHaveCount(0);

    await page.screenshot({ path: testInfo.outputPath("maloca_overview.png"), fullPage: true });
    expect(pageErrors).toEqual([]);
  });

  test("overview explains a non-JSON daemon response instead of a raw SyntaxError", async ({ page }) => {
    await page.route("**/maloca/pack", (route) =>
      route.fulfill({ status: 200, contentType: "text/html", body: "<!doctype html><html><body>spa</body></html>" }),
    );

    await page.goto("/#/maloca");

    const error = page.getByText(/System Error:/);
    await expect(error).toContainText("non-JSON response for /maloca/pack");
    await expect(error).not.toContainText("Unexpected token");
  });

  test("marketplace tab buys a listing and shows it as purchased", async ({ page }, testInfo) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));
    await page.route("**/maloca/pack", (route) => route.fulfill({ json: PACK }));
    const market = await mockMarketplace(page);

    await page.goto("/#/maloca");
    const tab = page.getByRole("button", { name: /^Marketplace/ });
    await tab.click();
    await expect(tab).toHaveAttribute("aria-current", "page");

    // Revoked listings are excluded from the buyer count but still shown as revoked.
    await expect(page.getByRole("button", { name: "Buyer Catalogue (1)" })).toBeVisible();
    await expect(page.getByText("Biology QA Dataset")).toBeVisible();
    await expect(page.getByText("Revoked by seller")).toBeVisible();

    await page.getByRole("button", { name: "Buy for 25 credits" }).click();
    await expect(page.getByText("Purchased")).toBeVisible();
    expect(market.purchases).toEqual([{ listing_id: "lst-biology-qa", buyer_id: "local-node" }]);

    await page.screenshot({ path: testInfo.outputPath("maloca_marketplace.png"), fullPage: true });
    expect(pageErrors).toEqual([]);
  });

  test("consent screen shows the node's current tier and keeps saving disabled without signing", async ({ page }) => {
    await page.route("**/maloca/pack", (route) => route.fulfill({ json: PACK }));
    await mockMarketplace(page);

    await page.goto("/#/maloca");
    await page.getByRole("button", { name: /^Marketplace/ }).click();
    await page.getByRole("button", { name: /Consent Screen \(LOCAL\)/ }).click();

    await expect(page.getByRole("button", { name: /Local Only/ })).toBeVisible();
    await expect(page.getByLabel("Terms Version")).toHaveValue("v1.0");
    // panel-ui has no client-side signing yet, so the signed consent write must stay disabled.
    await expect(page.locator('form button[type="submit"]').last()).toBeDisabled();
  });

  test("marketplace tab is hidden when the v2 routes are not mounted", async ({ page }) => {
    await page.route("**/maloca/pack", (route) => route.fulfill({ json: PACK }));
    await page.route("**/v2/marketplace/listings", (route) => route.fulfill({ status: 404, body: "" }));

    await page.goto("/#/maloca");
    await expect(page.getByRole("button", { name: /Mesh Registry/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Marketplace/ })).toHaveCount(0);
  });

  test("close button leaves the Maloca workspace", async ({ page }) => {
    await page.route("**/maloca/pack", (route) => route.fulfill({ json: PACK }));
    await page.goto("/#/maloca");
    await page.getByRole("button", { name: "Cerrar Maloca" }).click();
    await expect(page.getByRole("heading", { name: "Maloca Ops Hub" })).toHaveCount(0);
  });
});
