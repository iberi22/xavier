import { expect, test } from "@playwright/test";

test.describe("Auth Adversarial, Fuzzing & Edge-Cases E2E Suite", () => {
  test.beforeEach(async ({ page }) => {
    // Mock health
    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "ok" } });
    });

    // Mock providers config
    await page.route("**/v1/config/providers", async (route) => {
      await route.fulfill({
        json: {
          providers: [
            {
              provider: "local",
              model: "test-model",
              api_key: null,
              base_url: null,
            },
          ],
        },
      });
    });
  });

  test("rejects malformed email inputs with native validation", async ({
    page,
  }) => {
    await page.goto("/");
    const emailInput = page.locator('input[type="email"]');
    await expect(emailInput).toBeVisible();

    // Type completely invalid email string
    await emailInput.fill("not-an-email");
    const submitBtn = page.locator('button:has-text("INITIALIZE SESSION")');
    await submitBtn.click();

    // Browser HTML5 constraint validation should block submit
    const isValid = await emailInput.evaluate(
      (el: HTMLInputElement) => el.checkValidity(),
    );
    expect(isValid).toBe(false);
  });

  test("handles backend 500 error gracefully without crashing", async ({
    page,
  }) => {
    await page.route("**/auth/magic-link", async (route) => {
      await route.fulfill({
        status: 500,
        contentType: "application/json",
        body: JSON.stringify({
          ok: false,
          error: "Rate limit exceeded. Try again in 60s.",
        }),
      });
    });

    await page.goto("/");
    await page.fill('input[type="email"]', "attacker@botnet.local");
    await page.click('button:has-text("INITIALIZE SESSION")');

    // Assert red alert box appears with error message
    const errorBox = page.locator("text=Rate limit exceeded. Try again in 60s.");
    await expect(errorBox).toBeVisible();
    await expect(page.locator("h1:has-text('XAVIER LOGIN')")).toBeVisible();
  });

  test("toggles to password/2FA mode and rejects invalid 2FA code", async ({
    page,
  }) => {
    // Mock login returning requires_2fa: true
    await page.route("**/auth/login", async (route) => {
      const body = route.request().postDataJSON();
      if (!body.totp_code) {
        await route.fulfill({
          status: 401,
          contentType: "application/json",
          body: JSON.stringify({
            error: "mfa_required",
            message: "2FA code required",
          }),
        });
      } else if (body.totp_code !== "123456") {
        await route.fulfill({
          status: 401,
          contentType: "application/json",
          body: JSON.stringify({
            error: "mfa_invalid",
            message: "Invalid TOTP or backup code",
          }),
        });
      }
    });

    await page.goto("/");
    const toggleBtn = page.locator(
      'button:has-text("Otros métodos de acceso (Contraseña / 2FA)")',
    );
    await expect(toggleBtn).toBeVisible();
    await toggleBtn.click();

    // Now in password mode
    const passwordInput = page.locator('input[type="password"]');
    await expect(passwordInput).toBeVisible();

    await page.fill('input[type="email"]', "operator@xavier.local");
    await passwordInput.fill("super-secret-password");
    await page.click('button:has-text("INITIALIZE SESSION")');

    // 2FA code input must appear
    const twoFactorHeading = page.locator('text=Enter 2FA Code');
    await expect(twoFactorHeading).toBeVisible();

    // Type invalid 2FA code via keyboard
    await page.keyboard.type("999999");
    await page.click('button:has-text("INITIALIZE SESSION")');

    // Assert error is displayed
    await expect(
      page.locator("text=Invalid TOTP or backup code"),
    ).toBeVisible();
  });

  test("unauthenticated access to protected routes strictly fails-closed to login", async ({
    page,
  }) => {
    // Clear any leftover tokens
    await page.addInitScript(() => {
      window.sessionStorage.clear();
      window.localStorage.clear();
    });

    // Try navigating to protected routes directly
    for (const route of ["/#/telecom", "/#/maloca", "/#/master-key"]) {
      await page.goto(route);
      await page.waitForLoadState("domcontentloaded");
      // Must render login page
      await expect(
        page.locator("h1:has-text('XAVIER LOGIN')"),
      ).toBeVisible({ timeout: 5000 });
    }
  });

  test("corrupted or malformed session token in storage is cleared without crashing", async ({
    page,
  }) => {
    // Inject corrupt JSON into auth-storage and invalid token into sessionStorage
    await page.addInitScript(() => {
      window.localStorage.setItem("auth-storage", "{ malformed json... }}}");
      window.sessionStorage.setItem("xavier.rt", "CORRUPTED_TOKEN_###");
    });

    // Mock refresh failure on corrupted token
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({ error: "Invalid token structure" }),
      });
    });

    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // The app must recover and present LoginPage cleanly
    await expect(
      page.locator("h1:has-text('XAVIER LOGIN')"),
    ).toBeVisible();
  });

  test("blocks request when CAPTCHA Turnstile verification is rejected by edge gate", async ({
    page,
  }) => {
    await page.route("**/auth/magic-link", async (route) => {
      const body = route.request().postDataJSON();
      if (body.turnstile_token === "2x00000000000000000000AB" || !body.turnstile_token) {
        await route.fulfill({
          status: 403,
          contentType: "application/json",
          body: JSON.stringify({
            ok: false,
            error: "CAPTCHA verification failed. Security challenge invalid.",
          }),
        });
      } else {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ ok: true, magic_url: "/#/session?token=mock" }),
        });
      }
    });

    await page.goto("/");
    // Check turnstile container rendered
    const turnstileContainer = page.locator('[data-testid="turnstile-container"]');
    await expect(turnstileContainer).toBeVisible();

    await page.fill('input[type="email"]', "bot@spammer.net");
    await page.click('button:has-text("INITIALIZE SESSION")');

    // Assert user-friendly error from edge gate
    await expect(
      page.locator("text=CAPTCHA verification failed. Security challenge invalid."),
    ).toBeVisible();
    await expect(page.locator("h1:has-text('XAVIER LOGIN')")).toBeVisible();
  });
});
