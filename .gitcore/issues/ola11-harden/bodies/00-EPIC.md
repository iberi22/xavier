# [EPIC] Ola 11 — Harden Residuals

> Wave after Ola 10 Stabilize & Ship (#1098 closed).
> Labels: `ola11`, `wave-harden` — **NO `jules` on EPIC**.

## Goal
Close residuals left honest in Ola 10 and restore host `--lib` confidence while GitHub Actions minutes are scarce.

## Child issues
01 Headless `code_*` real handlers  
02 Fix `test_get_offline_status`  
03 Fix `test_overall_status_prioritization`  
04 Fix `test_reindex_null_embeddings_background`  
05 Fix `test_custom_dedup_policies` / PathExact threshold  
06 Align config/defaults `embedding_provider_mode` local-first  
07 Panel `/panel` missing-assets UX  
08 Onboarding auth without mandatory Tauri  
09 Docs: NixOS Docker / dockerd  
10 WiX remove `xavier-gui` residual  
11 Docs: local CI via agent-privilege-notify  
12 EPIC close (features + devlog) — orchestrator only

## Out of scope
- Ola 8 memory feature wave
- Mesh R&D (#115)
- Expanding Jules to edit `~/.hermes/skills` (document in-repo only)

## Definition of done
All children closed; `cargo test -p xavier --lib` 0 fail on host; features reconcile + devlog landed.
