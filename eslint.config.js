// Xavier root lint — Rust project, JS lint is scoped to panel-ui/docs via biome/pnpm filter.
// This file exists to satisfy ESLint v9 flat-config requirement so `pnpm run lint` doesn't fail with "couldn't find config".
// Root `src/` is Rust, not JS, so we ignore everything and delegate real JS lint to `pnpm --filter xavier-panel-ui run biome:check`.
export default [
  {
    ignores: ["**/*"]
  }
];
