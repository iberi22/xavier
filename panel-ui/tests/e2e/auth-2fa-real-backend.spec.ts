import { expect, test } from "@playwright/test";
import { generateTotp } from "./helpers/totp";

/**
 * REAL end-to-end coverage of the panel's /auth/* flow — no `page.route()` mocks.
 * Run against an actual `xavier http` process on an alternate port with a
 * disposable data/state dir (see playwright.auth-e2e.config.ts). Exercises the
 * full lifecycle described in docs/SWAL/PLAN_DESPLIEGUE_DIA_2026-09-24.md F2.2:
 *
 *   register -> login -> 2FA setup (QR + manual key) -> verify -> backup codes
 *   -> logout -> login with a live TOTP code -> login with a one-shot backup
 *   code -> account recovery via the 24-word seed phrase.
 */

test.describe.configure({ mode: "serial" });

test.describe("Panel 2FA — real backend (/auth/*)", () => {
  test.beforeEach(async ({ page }) => {
    // Skip the first-run onboarding wizard (gated on this localStorage flag in
    // App.tsx) — it is unrelated to auth and would otherwise shadow every route.
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "1");
    });
    page.on("pageerror", (err) => console.log(`[pageerror] ${err.message}`));
    page.on("response", async (res) => {
      if (res.url().includes("/auth/")) {
        let body = "";
        try {
          body = await res.text();
        } catch {
          body = "<unreadable>";
        }
        console.log(`[auth-response] ${res.request().method()} ${res.url()} -> ${res.status()} ${body.slice(0, 300)}`);
      }
    });
  });

  const stamp = Date.now();
  const email = `e2e-2fa-${stamp}@xavier.local`;
  const name = "E2E Operator";
  const password = "XavierE2E2026-Strong!";
  const newPassword = "XavierE2E2026-Recovered!";

  let seedPhrase = "";
  let totpSecret = "";
  let backupCodes: string[] = [];

  test("registro -> login -> setup 2FA -> verify -> backup codes -> logout -> login con TOTP -> backup code -> recuperacion", async ({
    page,
  }) => {
    // ---- 1. Registro ----
    await page.goto("/#/register");
    await page.locator("#register-name-input").fill(name);
    await page.locator("#register-email-input").fill(email);
    await page.locator("#register-password-input").fill(password);
    await page.locator("#register-confirm-password-input").fill(password);
    await page.getByRole("button", { name: "REGISTER ACCOUNT" }).click();

    await expect(page.getByText("Emergency Recovery Seed")).toBeVisible();
    const seedWords = await page
      .locator('[data-testid="seed-word"]')
      .allTextContents();
    expect(seedWords.length).toBe(24); // BIP39 Spanish, 24 words (new_account, src/auth2/mod.rs)
    seedPhrase = seedWords.join(" ");

    await page.getByRole("button", { name: "I HAVE SAVED MY WORDS" }).click();

    // ---- 2. Login (registration does not auto-authenticate) ----
    await expect(page.locator("#email-input")).toBeVisible();
    await page.locator("#email-input").fill(email);
    await page.locator("#password-input").fill(password);
    await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();

    // Hash was left at #/2fa/setup by the register step; once authenticated
    // App.tsx resolves that hash to TwoFactorSetup.
    await expect(
      page.getByText("2FA Configuration", { exact: false }),
    ).toBeVisible();

    // ---- 3. Setup 2FA: read the manual key (base32 secret) from the UI ----
    const secretLocator = page.locator('[data-testid="totp-manual-secret"]');
    await expect(secretLocator).toBeVisible();
    totpSecret = (await secretLocator.textContent())?.trim() ?? "";
    expect(totpSecret.length).toBeGreaterThan(0);

    const backupCodesBeforeVerify = await page
      .locator('[data-testid="backup-code"]')
      .count();
    expect(backupCodesBeforeVerify).toBe(0); // not on this page yet

    // ---- 4. Verify with a real computed TOTP code ----
    let code = generateTotp(totpSecret);
    await page.locator('input[maxlength="6"]').fill(code, { force: true });
    const activateButton = page.getByRole("button", { name: "ACTIVATE 2FA" });
    await activateButton.click();

    // If the 30s window rolled over between reading the secret and clicking,
    // retry once with a freshly computed code instead of flaking.
    const invalidCode = page.getByText("Invalid code. Please try again.");
    if (await invalidCode.isVisible().catch(() => false)) {
      code = generateTotp(totpSecret);
      await page.locator('input[maxlength="6"]').fill(code, { force: true });
      await activateButton.click();
    }

    // ---- 5. Backup codes shown once ----
    await expect(page.getByText("Backup Access Codes")).toBeVisible();
    backupCodes = await page
      .locator('[data-testid="backup-code"]')
      .allTextContents();
    expect(backupCodes.length).toBe(10);
    await page.getByRole("button", { name: "I HAVE SAVED MY CODES" }).click();

    // Backup codes must not be re-fetchable via another setup2FA() call — the
    // pending-codes store slot is cleared on acknowledgement (BackupCodesPage).
    await page.goto("/#/2fa/backup");
    const staleCodes = await page.locator('[data-testid="backup-code"]').count();
    expect(staleCodes).toBe(0);

    // ---- 6. Logout ----
    await page.goto("/#/master-key");
    await page.getByTestId("logout-button").click();
    await expect(page.locator("#email-input")).toBeVisible();

    // ---- 7. Login again: password alone must now prompt for a 2FA code ----
    await page.goto("/#/login");
    await page.locator("#email-input").fill(email);
    await page.locator("#password-input").fill(password);
    await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();
    await expect(page.getByText("Enter 2FA Code")).toBeVisible();

    const totpCode = generateTotp(totpSecret);
    // LoginPage's TwoFactorInput accepts up to 8 chars (6-digit TOTP or 8-digit
    // backup code both land on /auth/login) — see components/TwoFactorInput.tsx.
    await page.locator('input[maxlength="8"]').fill(totpCode, { force: true });
    await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();

    // Authenticated: TOTP login lands back on the dashboard (hash reset to "#/master-key"?
    // no explicit reset on login, so we just assert we're no longer on the login form).
    await expect(page.locator("#email-input")).not.toBeVisible();

    // ---- 8. Logout again, then login using a one-shot backup code ----
    await page.goto("/#/master-key");
    await page.getByTestId("logout-button").click();
    await expect(page.locator("#email-input")).toBeVisible();

    await page.locator("#email-input").fill(email);
    await page.locator("#password-input").fill(password);
    await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();
    await expect(page.getByText("Enter 2FA Code")).toBeVisible();

    const backupCode = backupCodes[0];
    expect(backupCode).toBeTruthy();
    await page.locator('input[maxlength="8"]').fill(backupCode, { force: true });
    await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();
    await expect(page.locator("#email-input")).not.toBeVisible();

    // A consumed backup code must not work a second time (one-shot use).
    await page.goto("/#/master-key");
    await page.getByTestId("logout-button").click();
    await page.locator("#email-input").fill(email);
    await page.locator("#password-input").fill(password);
    await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();
    await expect(page.getByText("Enter 2FA Code")).toBeVisible();
    await page.locator('input[maxlength="8"]').fill(backupCode, { force: true });
    await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();
    // login_handler (src/auth2/mod.rs) rejects the reused code as "mfa_invalid"
    // ("codigo 2FA invalido") — the LoginPage catch block surfaces it verbatim.
    await expect(page.locator("text=/codigo 2FA invalido/i")).toBeVisible();
    await expect(page.locator("#email-input")).toBeVisible();

    // ---- 9. Recovery via the 24-word seed phrase (also disables 2FA) ----
    await page.goto("/#/recovery");
    await page.locator("#recovery-email-input").fill(email);
    await page.locator("#recovery-seed-phrase-input").fill(seedPhrase);
    await page.locator("#recovery-new-password-input").fill(newPassword);
    await page.locator("#recovery-confirm-password-input").fill(newPassword);
    await page.getByRole("button", { name: "RESET PASSWORD" }).click();
    await expect(page.getByText("Access Restored")).toBeVisible();
    await page.getByRole("button", { name: "BACK TO LOGIN" }).click();

    // Recovery disables 2FA server-side (recovery_handler) — plain password login works now.
    await expect(page.locator("#email-input")).toBeVisible();
    await page.locator("#email-input").fill(email);
    await page.locator("#password-input").fill(newPassword);
    await page.getByRole("button", { name: "INITIALIZE SESSION" }).click();
    await expect(page.locator("#email-input")).not.toBeVisible();
    await expect(page.getByText("Enter 2FA Code")).not.toBeVisible();
  });
});
