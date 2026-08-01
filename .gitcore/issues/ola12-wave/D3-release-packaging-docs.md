# D3: Release packaging documentation

## Problem

Windows packaging (WiX) was simplified in Ola 11 (#1138) — removed
`xavier-gui.exe`, shortcuts now point to `xavier.exe`. But there's no
documented release pathway for building installers.

## Solution

Document the complete release packaging process.

### Steps

1. Document WiX build process in `docs/ops/release-packaging.md`
2. Document Tauri build for Linux/macOS
3. Document cross-compilation from NixOS to Windows (if feasible)
4. Add `cargo xbuild` or `cross` configuration if needed
5. Create `scripts/release-build.sh` that orchestrates the build

## Acceptance

- [ ] `docs/ops/release-packaging.md` exists with full instructions
- [ ] WiX build steps documented (Windows)
- [ ] Tauri build steps documented (Linux/macOS)
- [ ] Cross-compilation notes (NixOS → Windows)
- [ ] `scripts/release-build.sh` exists (optional)

## Files

- `docs/ops/release-packaging.md` (new)
- `scripts/release-build.sh` (new, optional)
