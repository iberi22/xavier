import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const xavierTarget =
  process.env.XAVIER_WEB_PROXY_TARGET ?? "http://127.0.0.1:8006";

export default defineConfig(({ command }) => {
  const _isBuild = command === "build";

  return {
    define: {
      __APP_VERSION__: JSON.stringify(
        process.env.npm_package_version || "0.6.1-beta",
      ),
    },
    base: "/",
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
        "/maloca": {
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
        "/auth": {
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
        "/maloca": {
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
