import fs from "node:fs";
import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const xavierTarget =
  process.env.XAVIER_WEB_PROXY_TARGET ?? "http://127.0.0.1:8006";

const cargoContent = (() => {
  try {
    return fs.readFileSync(path.resolve(__dirname, "../Cargo.toml"), "utf8");
  } catch {
    try {
      return fs.readFileSync("../../Cargo.toml", "utf8");
    } catch {
      return "";
    }
  }
})();
const appVersion =
  cargoContent.match(/^version = "(.+)"/m)?.[1] ??
  process.env.npm_package_version ??
  "0.0.1";

function proxyRoutesFor(target: string) {
  return {
    "/health": { target, changeOrigin: true },
    "/maloca": { target, changeOrigin: true },
    "/panel/api": { target, changeOrigin: true },
    "/v1": { target, changeOrigin: true },
    "/auth": { target, changeOrigin: true },
    "/api": { target, changeOrigin: true },
    "/notifications": { target, changeOrigin: true },
  };
}

// `vite dev` (pnpm dev): proxies to a real backend by default (XAVIER_WEB_PROXY_TARGET,
// falling back to the conventional local dev port 8006) — unchanged, pre-existing behavior
// for local development.
const devProxyRoutes = proxyRoutesFor(xavierTarget);

// `vite preview` (used by the Playwright e2e webServer, both playwright.config.ts's mocked
// suite and playwright.auth-e2e.config.ts's real-backend suite): only proxy when
// XAVIER_WEB_PROXY_TARGET is EXPLICITLY set (as playwright.auth-e2e.config.ts does, pointing
// at its own disposable instance). Falling back to :8006 here — a real, possibly-production
// xavier a developer happens to have running locally — silently let unmocked requests in the
// DEFAULT mocked e2e suite (playwright.config.ts, no XAVIER_WEB_PROXY_TARGET) leak to it
// instead of failing fast like they did before this proxy existed. Regression found via
// onboarding.spec.ts flipping from pass (origin/main, no preview proxy at all) to fail
// (this branch) when run against a machine with xavier.service active on :8006.
const previewProxyRoutes = process.env.XAVIER_WEB_PROXY_TARGET
  ? proxyRoutesFor(process.env.XAVIER_WEB_PROXY_TARGET)
  : undefined;

export default defineConfig(({ command }) => {
  const _isBuild = command === "build";

  return {
    define: {
      __APP_VERSION__: JSON.stringify(appVersion),
    },
    base: "/",
    assetsInclude: ["**/*.wasm"],
    optimizeDeps: {
      include: ["@swal/maloca-embed"],
    },
    plugins: [tailwindcss(), react()],
    resolve: {
      alias: {
        "@openuidev/react-headless": path.resolve(
          __dirname,
          "./node_modules/@openuidev/react-headless/dist/index.js",
        ),
        zustand: path.resolve(__dirname, "./node_modules/zustand"),
        "zustand/react/shallow": path.resolve(
          __dirname,
          "./node_modules/zustand/react/shallow.js",
        ),
        "@swal/maloca-wasm/main": path.resolve(
          __dirname,
          "../../maloca/packages/maloca-wasm/main.js",
        ),
        "@swal/maloca-wasm": path.resolve(
          __dirname,
          "../../maloca/packages/maloca-wasm",
        ),
      },
    },
    server: {
      host: "127.0.0.1",
      port: 4174,
      proxy: devProxyRoutes,
    },
    preview: {
      host: "127.0.0.1",
      port: 4174,
      proxy: previewProxyRoutes,
    },
    build: {
      outDir: "build",
      emptyOutDir: true,
      assetsDir: "assets",
      rollupOptions: {
        external: ["@swal/maloca-wasm", "@swal/maloca-wasm/main"],
        output: {
          entryFileNames: "assets/index.js",
          chunkFileNames: "assets/[name].js",
          assetFileNames: (assetInfo) => {
            if (assetInfo.name?.endsWith(".css")) {
              return "assets/index.css";
            }
            return "assets/[name][extname]";
          },
        },
      },
    },
  };
});
