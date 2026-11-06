import { test, expect, type Page, type WebSocketRoute } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const DEFAULT_ARTIFACT_DIR = path.join(process.cwd(), "test-results", "artifacts");

function getWritableDir(target: string): string {
  try {
    fs.mkdirSync(target, { recursive: true });
    fs.accessSync(target, fs.constants.W_OK);
    return target;
  } catch {
    fs.mkdirSync(DEFAULT_ARTIFACT_DIR, { recursive: true });
    return DEFAULT_ARTIFACT_DIR;
  }
}

const ARTIFACT_DIR = getWritableDir(process.env.ARTIFACT_DIR || DEFAULT_ARTIFACT_DIR);

test.describe("Telecom File Transfer Drawer E2E", () => {
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
    // this test navigates straight to a gated route (#/telecom/direct) with no explicit login
    // click, so isAuthenticated only ever becomes true via this call. Shape matches the real
    // backend (refresh_handler's AuthResponse, src/auth2/mod.rs): { access_token, refresh_token }.
    await page.route("**/auth/refresh", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          access_token: "mock.jwt.access-token-file-transfer",
          refresh_token: "mock-refresh-token-file-transfer",
        }),
      });
    });

    // Bypass onboarding and set authenticated session
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
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

  /**
   * Mocks the telecom gateway socket (`/telecom/ws/:client_id`, see useTelecomSocket) and
   * resolves with the most recent connection so the test can push daemon frames.
   */
  async function mockTelecomSocket(page: Page) {
    let latest: WebSocketRoute | null = null;
    await page.routeWebSocket(/\/telecom\/ws\//, (ws) => {
      latest = ws;
      // Heartbeat pings from the client are accepted and ignored.
      ws.onMessage(() => {});
    });
    return {
      async send(frame: Record<string, unknown>) {
        await expect.poll(() => latest !== null).toBe(true);
        latest!.send(JSON.stringify(frame));
      },
    };
  }

  async function startUpload(page: Page) {
    await page.goto("/#/telecom/direct");
    await page.locator('input[type="file"]').first().setInputFiles({
      name: "TopSecret_Report.pdf",
      mimeType: "application/pdf",
      buffer: Buffer.from("mock content"),
    });
    await page.getByRole("button", { name: "Send direct message" }).click();
    const dialog = page.getByRole("dialog", { name: /File Transfer Drawer/ });
    await expect(dialog).toBeVisible();
    return dialog;
  }

  // Frame shape mirrors the Rust TransferProgress struct
  // { session_id, received_chunks, total_chunks, bytes_transferred, is_completed }.
  const progressFrame = (received: number, total: number, isCompleted: boolean) => ({
    event_type: "file_transfer_progress",
    payload: {
      session_id: "xfer-e2e-1",
      received_chunks: received,
      total_chunks: total,
      bytes_transferred: received * 1024,
      is_completed: isCompleted,
    },
  });

  test("drives the drawer from telecom socket progress frames to completion", async ({ page }) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));
    const socket = await mockTelecomSocket(page);

    const dialog = await startUpload(page);
    await expect(dialog.locator("#file-transfer-drawer-title")).toHaveText("File Transfer Drawer");
    await expect(dialog.getByText("TopSecret_Report.pdf").first()).toBeVisible();

    const progressBar = dialog.getByRole("progressbar");
    await expect(progressBar).toHaveAttribute("aria-valuenow", "0");

    await socket.send(progressFrame(5, 10, false));
    await expect(progressBar).toHaveAttribute("aria-valuenow", "50");
    await expect(dialog.getByText("5 / 10 chunks transferred")).toBeVisible();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "file_transfer_drawer_active.png"), fullPage: true });

    await socket.send(progressFrame(10, 10, true));
    await expect(progressBar).toHaveAttribute("aria-valuenow", "100");
    await expect(dialog.getByText("10 / 10 chunks transferred")).toBeVisible();
    await expect(dialog.getByText("Verified Integrity")).toBeVisible();
    await expect(dialog.getByRole("button", { name: /Open Transferred File/ })).toBeVisible();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "file_transfer_drawer_completed.png"), fullPage: true });

    await dialog.getByRole("button", { name: "Close transfer drawer" }).click();
    await expect(dialog).toBeHidden();
    expect(pageErrors).toEqual([]);
  });

  test("shows the failed state when the socket reports file_transfer_failed", async ({ page }) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(String(err)));
    const socket = await mockTelecomSocket(page);

    const dialog = await startUpload(page);
    await socket.send(progressFrame(3, 10, false));
    await expect(dialog.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "30");

    await socket.send({
      event_type: "file_transfer_failed",
      payload: { session_id: "xfer-e2e-1", reason: "checksum mismatch" },
    });
    await expect(dialog.getByText("Checksum Mismatch")).toBeVisible();
    await expect(dialog.getByText("Verified Integrity")).toHaveCount(0);
    await expect(dialog.getByRole("button", { name: /Awaiting Completion/ })).toBeDisabled();
    await page.screenshot({ path: path.join(ARTIFACT_DIR, "file_transfer_drawer_failed.png"), fullPage: true });
    expect(pageErrors).toEqual([]);
  });
});
