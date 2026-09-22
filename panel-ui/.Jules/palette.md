## 2024-03-24 - DraggableWidget SVGs missing title

**Learning:** Purely decorative SVGs in React components (like the simulated graphs in `DraggableWidget`) will fail the `@biomejs/biome` `lint/a11y/noSvgWithoutTitle` rule.
**Action:** When adding or modifying SVGs that serve purely visual layout or decorative purposes, always apply `aria-hidden="true"` to them to fix linting and improve screen reader experiences by hiding non-semantic content.
