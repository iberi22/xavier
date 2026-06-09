import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const xavierTarget = process.env.XAVIER_WEB_PROXY_TARGET ?? "http://127.0.0.1:8006";

export default defineConfig(({ command }) => {
  const isBuild = command === "build";

  return {
    define: {
      __APP_VERSION__: JSON.stringify(process.env.npm_package_version || "0.6.1-beta"),
    },
    base: isBuild ? "/panel/" : "/",
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
