# Ola 11 — Harden Residuals

**Parent EPIC:** (created on GitHub)  
**Theme:** Close honest deferrals from Ola 10; make `--lib` green; panel/packaging/ops truth.  
**Depends on:** Ola 10 closed (#1098).

See `FILE_OWNERSHIP.md` and `bodies/` for Jules-ready issue text.

## Dispatch rules
1. Create issues **without** `jules`
2. Run island harness (no overlapping paths)
3. Apply `jules` to 01–11 in one batch (12 never gets `jules`)
4. Merge with diff review if GHA budget exhausted
5. Issue 12 last (orchestrator)

## Success
- `cargo test -p xavier --lib` → 0 failed
- Headless `code_scan` returns real results (not 501)
- `/panel` returns helpful HTML stub or assets (never silent empty)
- WiX no longer references missing `xavier-gui.exe`
- Ops docs for NixOS dockerd + local CI via agent-priv
