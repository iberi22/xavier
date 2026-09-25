import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

// panel-ui's package.json sets "type": "module", so __dirname isn't available.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Dedicated Playwright config for the REAL-backend 2FA e2e suite
 * (tests/e2e/auth-2fa-real-backend.spec.ts).
 *
 * Unlike playwright.config.ts (which mocks every `/v1/auth` or `/auth` call via
 * page.route()), this config boots an actual `xavier http` process against a
 * disposable, throwaway data/state directory on an alternate port — never the
 * operator's real `~/.xavier` and never the production instance on :8006.
 *
 * Required before running: `pnpm run build` (so `vite preview` has a `build/`
 * dir to serve) and a compiled `xavier` binary (`cargo build --bin xavier`
 * from the repo root, or set XAVIER_E2E_BIN to an existing binary path).
 */

const baseURL = process.env.PANEL_UI_BASE_URL ?? "http://127.0.0.1:4174";
const backendPort = process.env.XAVIER_E2E_PORT ?? "18016";
const backendUrl = `http://127.0.0.1:${backendPort}`;

// Fresh, disposable state+data dir per run.
const stateDir =
  process.env.XAVIER_E2E_STATE_DIR ??
  fs.mkdtempSync(path.join(os.tmpdir(), "xavier-panel-auth-e2e-"));

const xavierBin =
  process.env.XAVIER_E2E_BIN ??
  path.resolve(__dirname, "../target/debug/xavier");

// NixOS has no global FHS lib path, so Playwright's downloaded (non-Nix) chromium
// binary can't find its shared libs (libnspr4, libnss3, libdbus-1, glib, libgbm)
// at their usual locations. Scoped to the browser subprocess only (via
// launchOptions.env below), never the Node/Playwright runner itself — an earlier
// attempt at exporting this shell-wide picked up a stray libc from an FHS
// sandbox dir and broke `node`. Override via XAVIER_E2E_LD_LIBRARY_PATH if these
// store paths drift (e.g. after a nix-channel update).
const chromiumLdLibraryPath =
  process.env.XAVIER_E2E_LD_LIBRARY_PATH ??
  [
    "/nix/store/ydlg1n7mb8la56x1yafd9nahnz42bpvz-system-path/lib", // libnspr4, libnss3
    "/nix/store/yanmwp5f435ing2nbhwa4v0gdmpl2an1-dbus-1.16.2-lib/lib", // libdbus-1
    "/nix/store/0iksbi3kkh2af1jv5zzf7jx1f0rxh201-glib-2.86.3/lib", // libglib/libgobject/libgio
    "/nix/store/h203q41kcnhjk9sgimxrh8ahpnxs80ql-mesa-libgbm-26.1.3/lib", // libgbm
  ].join(":");

export default defineConfig({
  testDir: "./tests",
  testMatch: "e2e/auth-2fa-real-backend.spec.ts",
  timeout: 90_000,
  expect: { timeout: 15_000 },
  retries: 0,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL,
    actionTimeout: 10_000,
    viewport: { width: 1280, height: 720 },
    trace: "on-first-retry",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
    launchOptions: {
      env: {
        ...(process.env as Record<string, string>),
        LD_LIBRARY_PATH: chromiumLdLibraryPath,
      },
    },
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: [
    {
      command: `${xavierBin} http ${backendPort} --host 127.0.0.1 --mcp-port 0 --no-ui`,
      url: `${backendUrl}/health`,
      reuseExistingServer: false,
      timeout: 60_000,
      // XAVIER_STATE_DIR (auth2 db, `.xavier/auth.db`) defaults to $HOME when unset —
      // that is the SAME path the production xavier.service instance uses, so it MUST
      // be overridden here or this test would create/mutate real operator accounts.
      env: {
        XAVIER_STATE_DIR: stateDir,
        XAVIER_DATA_DIR: stateDir,
        // Isolates the /auth/* JWT keypair + DB master key from the machine's real
        // "xavier-auth" OS keyring / ~/.xavier/secrets entries, which are NOT scoped
        // by XAVIER_STATE_DIR/XAVIER_DATA_DIR (see auth2::auth_vault_service_name).
        XAVIER_AUTH_VAULT_SERVICE: `xavier-auth-e2e-${Date.now()}`,
      },
    },
    {
      command: "npx vite preview --port 4174 --host 127.0.0.1",
      url: baseURL,
      reuseExistingServer: !process.env.CI,
      timeout: 60_000,
      env: {
        XAVIER_WEB_PROXY_TARGET: backendUrl,
      },
    },
  ],
});
