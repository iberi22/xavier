// @ts-check

import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";

// https://astro.build/config
export default defineConfig({
  site: "https://southwest-ai-labs.github.io",
  base: "/xavier",
  integrations: [
    starlight({
      title: "Xavier",
      description: "Cognitive Memory for AI Swarms",
      pagefind: false,
      sidebar: [
        {
          label: "Getting Started",
          items: [
            { label: "Introduction", link: "/guides/intro/" },
            { label: "Installation", link: "/guides/installation/" },
            { label: "Quick Start", link: "/guides/quick-start/" },
          ],
        },
        {
          label: "Architecture",
          items: [{ autogenerate: { directory: "architecture" } }],
        },
        {
          label: "Modules",
          items: [{ autogenerate: { directory: "modules" } }],
        },
        {
          label: "Features",
          items: [{ autogenerate: { directory: "features" } }],
        },
        {
          label: "API Reference",
          items: [{ autogenerate: { directory: "reference" } }],
        },
        {
          label: "Testing",
          items: [{ autogenerate: { directory: "testing" } }],
        },
      ],
      customCss: ["./src/styles/custom.css"],
    }),
  ],
});
