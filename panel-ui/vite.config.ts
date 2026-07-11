import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const xavierTarget =
  process.env.XAVIER_WEB_PROXY_TARGET ?? "http://127.0.0.1:8006";

export default defineConfig(({ command }) => {
  const isBuild = command === "build";
  // Tauri embeds the frontend at the web root (tauri://localhost/), while the
  // standalone Axum backend serves it under /panel. Detect the Tauri build via
  // any of the env vars its CLI injects and only use the /panel base for the
  // web bundle consumed by the Axum server.
  const isTauriBuild = !!(
    process.env.TAURI_ENV_PLATFORM ||
    process.env.TAURI_PLATFORM ||
    process.env.TAURI_ENV_ARCH
  );

  return {
    define: {
      __APP_VERSION__: JSON.stringify(
        process.env.npm_package_version || "0.6.1-beta",
      ),
    },
    // Backend serves the web panel at /panel (assets under /panel/assets/*).
    // Tauri serves the same bundle from the root, so keep base "/" for it.
    // The dev server also stays at "/" (its proxy already routes /panel/api).
    base: isBuild && !isTauriBuild ? "/panel/" : "/",
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
      },
    },
    server: {
      host: "127.0.0.1",
      port: 4174,
      proxy: {
        "/health": {
          target: xavierTarget,
          changeOrigin: true,
        },
        "/panel/api": {
          target: xavierTarget,
          changeOrigin: true,
        },
        "/v1": {
          target: xavierTarget,
          changeOrigin: true,
        },
        "/api": {
          target: xavierTarget,
          changeOrigin: true,
        },
        "/notifications": {
          target: xavierTarget,
          changeOrigin: true,
        },
      },
    },
    build: {
      outDir: "build",
      emptyOutDir: true,
      assetsDir: "assets",
      rollupOptions: {
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
