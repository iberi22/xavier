import node from "@astrojs/node";
import svelte from "@astrojs/svelte";
import { defineConfig } from "astro/config";

export default defineConfig({
  integrations: [svelte()],
  output: "static",
  adapter: node({
    mode: "standalone",
  }),
  build: {
    format: "file",
  },
  vite: {
    ssr: {
      noExternal: ["flexsearch"],
    },
  },
});
