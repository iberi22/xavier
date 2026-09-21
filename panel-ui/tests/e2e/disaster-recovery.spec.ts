import { expect, test } from "@playwright/test";
import path from "node:path";
import fs from "node:fs";

const ARTIFACT_DIR = path.join(process.cwd(), "test-results", "artifacts");

test.describe("Sovereign Recovery Modal & Disaster Recovery Wizard E2E", () => {
  test.beforeEach(async ({ page, context }) => {
    // Grant clipboard permissions for Chromium testing
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);

    // Mock Genesis Node endpoints & health API
    await page.route("**/v1/auth/challenge", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ok: true,
          challenge_id: "test-challenge-uuid-recovery",
          nonce_hex: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
          issued_at: Math.floor(Date.now() / 1000),
          expires_at: Math.floor(Date.now() / 1000) + 60,
          domain: "swal-mesh-signed-nonce-v1",
          genesis_node: "xaviercloud.swal.network",
        }),
      });
    });

    await page.route("**/v1/auth/verify", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ok: true,
          token: "swal_sess_mock_recovery_token_999",
          node_id: "node_ed0fe57d1f71cbf6",
          karma: 50,
          genesis_node: "xaviercloud.swal.network",
          genesis_status: "connected",
          role: "node_peer",
        }),
      });
    });

    await page.route("**/health", async (route) => {
      await route.fulfill({ json: { status: "online", service: "xaviercloud" } });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({ json: [] });
    });

    // Seed local storage with authenticated session and skip onboarding
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_token", "swal_sess_mock_recovery_token_999");
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem(
        "swal_node_session_v1",
        JSON.stringify({
          nodeId: "node_ed0fe57d1f71cbf6",
          token: "swal_sess_mock_recovery_token_999",
          karma: 50,
          genesisNode: "xaviercloud.swal.network",
          genesisStatus: "connected",
          connectedAt: Date.now(),
        })
      );
    });

    // Ensure artifact directory exists
    if (!fs.existsSync(ARTIFACT_DIR)) {
      fs.mkdirSync(ARTIFACT_DIR, { recursive: true });
    }
  });

  test("opens Disaster Recovery Wizard and inspects Backup Node tab & copy phrase interaction", async ({
    page,
  }) => {
    await page.goto("/#/wallet");

    // Ensure WalletView is loaded
    await expect(page.locator("h2:has-text('Billetera Soberana SWAL')")).toBeVisible({
      timeout: 10000,
    });

    // Click on Recovery BIP-39 button to open RecoveryModal
    const recoveryBtn = page.locator("button:has-text('Recuperación BIP-39 (Share 2)')");
    await expect(recoveryBtn).toBeVisible();
    await recoveryBtn.click();

    // Verify Disaster Recovery Wizard modal header and zero-knowledge banner
    const modalHeader = page.locator("h2:has-text('Disaster Recovery Wizard')");
    await expect(modalHeader).toBeVisible({ timeout: 5000 });
    await expect(
      page.locator("span:has-text('Zero-Knowledge Sovereign Custody Notice')")
    ).toBeVisible();

    // Verify 12 seed phrase words are rendered in the grid
    const mnemonicWords = [
      "abandon",
      "amount",
      "abandon",
      "amount",
      "abandon",
      "amount",
      "abandon",
      "amount",
      "abandon",
      "amount",
      "abandon",
      "announce",
    ];

    for (const word of mnemonicWords) {
      await expect(page.locator(`span:has-text('${word}')`).first()).toBeVisible();
    }

    // Test "Copy Seed Phrase" button interaction
    const copyBtn = page.locator("button:has-text('Copy Seed Phrase')");
    await expect(copyBtn).toBeVisible();
    await copyBtn.click();

    // Check feedback text changes to "Copied to Clipboard"
    await expect(page.locator("button:has-text('Copied to Clipboard')")).toBeVisible();

    // Capture screenshot of Backup View
    const backupScreenshotPath = path.join(ARTIFACT_DIR, "disaster_recovery_backup_e2e.png");
    await page.screenshot({ path: backupScreenshotPath, fullPage: true });
  });

  test("verifies interactive 2-word backup quiz validation logic", async ({ page }) => {
    await page.goto("/#/wallet");

    await expect(page.locator("h2:has-text('Billetera Soberana SWAL')")).toBeVisible({
      timeout: 10000,
    });

    const recoveryBtn = page.locator("button:has-text('Recuperación BIP-39 (Share 2)')");
    await recoveryBtn.click();

    await expect(page.locator("h2:has-text('Disaster Recovery Wizard')")).toBeVisible();

    // Locate quiz prompt text containing word numbers (e.g. "please type word #1 and word #2.")
    const quizPromptLocator = page.locator("p:has-text('please type word')");
    await expect(quizPromptLocator).toBeVisible();
    const quizPromptText = await quizPromptLocator.textContent();

    // Extract word indices from numbers in prompt
    const matches = quizPromptText?.match(/#(\d+)/g);
    expect(matches).not.toBeNull();
    expect(matches?.length).toBeGreaterThanOrEqual(2);

    const idx1 = parseInt(matches![0].replace("#", ""), 10) - 1;
    const idx2 = parseInt(matches![1].replace("#", ""), 10) - 1;

    const fullMnemonic =
      "abandon amount abandon amount abandon amount abandon amount abandon amount abandon announce";
    const wordsList = fullMnemonic.split(" ");
    const expectedWord1 = wordsList[idx1];
    const expectedWord2 = wordsList[idx2];

    const input1 = page.locator(`input[placeholder*='Enter word #${idx1 + 1}']`);
    const input2 = page.locator(`input[placeholder*='Enter word #${idx2 + 1}']`);

    // Submit invalid quiz inputs first
    await input1.fill("wrongword1");
    await input2.fill("wrongword2");

    // Verify error message and disabled confirmation button
    await expect(
      page.locator("text=Words do not match your backup mnemonic phrase.")
    ).toBeVisible();

    const confirmBtn = page.locator("button:has-text('I have securely backed up my key')");
    await expect(confirmBtn).toBeDisabled();

    // Fill correct quiz answers
    await input1.fill(expectedWord1);
    await input2.fill(expectedWord2);

    // Verify success message and enabled button
    await expect(page.locator("text=Word verification passed!")).toBeVisible();
    await expect(confirmBtn).toBeEnabled();

    // Click confirm button and verify modal closes
    await confirmBtn.click();
    await expect(page.locator("h2:has-text('Disaster Recovery Wizard')")).not.toBeVisible();
  });

  test("switches to Restore Node tab, tests mnemonic error states, and submits valid phrase", async ({
    page,
  }) => {
    await page.goto("/#/wallet");

    await expect(page.locator("h2:has-text('Billetera Soberana SWAL')")).toBeVisible({
      timeout: 10000,
    });

    const recoveryBtn = page.locator("button:has-text('Recuperación BIP-39 (Share 2)')");
    await recoveryBtn.click();

    await expect(page.locator("h2:has-text('Disaster Recovery Wizard')")).toBeVisible();

    // Switch to "Restore Node" tab
    const restoreTab = page.locator("button:has-text('Restore Node')");
    await expect(restoreTab).toBeVisible();
    await restoreTab.click();

    // Verify Restore banner text
    await expect(
      page.locator("span:has-text('Sovereign Node Reconstruction')")
    ).toBeVisible();

    const textarea = page.locator("textarea[placeholder*='e.g. abandon amount']");
    await expect(textarea).toBeVisible();

    const restoreSubmitBtn = page.locator(
      "button:has-text('Restore Sovereign Node Identity')"
    );

    // 1. Enter invalid length (e.g. 5 words)
    await textarea.fill("abandon amount abandon amount abandon");
    await restoreSubmitBtn.click();

    await expect(
      page.locator(
        "text=Invalid seed phrase length: 5 words found. Must be exactly 12 or 24 words."
      )
    ).toBeVisible();

    // 2. Enter invalid characters (e.g. 12 words but with numbers/symbols)
    await textarea.fill(
      "abandon amount 1234!! amount abandon amount abandon amount abandon amount abandon announce"
    );
    await restoreSubmitBtn.click();

    await expect(
      page.locator("text=Seed phrase contains invalid characters or words")
    ).toBeVisible();

    // 3. Enter valid 12-word BIP-39 mnemonic phrase
    const validMnemonic =
      "abandon amount abandon amount abandon amount abandon amount abandon amount abandon announce";
    await textarea.fill(validMnemonic);
    await restoreSubmitBtn.click();

    // Verify modal closes upon successful restore submission
    await expect(
      page.locator("h2:has-text('Disaster Recovery Wizard')")
    ).not.toBeVisible();

    // Capture screenshot of Restore View before submission
    // Re-open to capture artifact
    await recoveryBtn.click();
    await restoreTab.click();
    await textarea.fill(validMnemonic);
    const restoreScreenshotPath = path.join(ARTIFACT_DIR, "disaster_recovery_restore_e2e.png");
    await page.screenshot({ path: restoreScreenshotPath, fullPage: true });
  });
});
