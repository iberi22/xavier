# D2: Panel UI build smoke test

## Problem

`panel-ui/` has an HTML stub from Ola 11 (#1135) but no build verification.
The panel assets are not tested — we don't know if `pnpm build` works or
if the output is functional.

## Solution

Verify and document the panel UI build pipeline.

### Steps

1. Run `cd panel-ui && pnpm install && pnpm build`
2. Verify output in `panel-ui/dist/` (HTML, JS, CSS)
3. Smoke test: serve dist/ locally and verify page loads
4. Document build path in `docs/ops/panel-build.md`
5. Add build verification to CI pipeline (D1)

## Acceptance

- [ ] `pnpm build` completes without errors
- [ ] `dist/` contains index.html + assets
- [ ] Page loads in browser (manual check or curl)
- [ ] Build documented in ops docs
- [ ] CI script includes panel build check

## Files

- `panel-ui/dist/` (generated)
- `docs/ops/panel-build.md` (new)
