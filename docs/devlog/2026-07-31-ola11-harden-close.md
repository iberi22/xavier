# DevLog — Ola 11 Harden Residuals (close)

**Date:** 2026-07-31  
**EPIC:** [#1128](https://github.com/iberi22/xavier/issues/1128)

## Why

Ola 10 left honest deferrals (headless `code_*` 501, five host `--lib` fails, panel/ops packaging gaps). Ola 11 closed them without waiting on Jules.

## Outcomes

| Issue | Result |
|-------|--------|
| #1129 headless code_* | Real scan/find/stats/context via code-graph |
| #1130 offline_models | Hermetic stopped-port status test |
| #1131 health prioritization | Hermetic status aggregation unit test |
| #1132 reindex embeddings | Delete+insert + verify vec row |
| #1133 PathExact dedup | Recompute cosine; fixture + assertions fixed |
| #1134 settings local-first | config + defaults `embedding_provider_mode=local` (+ local embed endpoint) |
| #1135 panel 503 | HTML stub with build/`XAVIER_PANEL_UI_DIR` guidance |
| #1136 onboarding | Skip Tauri `invoke` on web complete; AuthStep stays HTTP |
| #1137/#1139 ops docs | `docs/ops/nixos-docker.md`, `local-ci-with-agent-priv.md` |
| #1138 WiX | Removed `xavier-gui.exe`; shortcut → `xavier.exe` |

## Verification

```text
cargo test -p xavier --lib -- \
  test_custom_dedup_policies test_load_config_json test_get_offline_status \
  test_overall_status_prioritization test_reindex_null_embeddings_background \
  test_execute_tool_code_scan_returns_real_result
→ 8 passed
```

## Next

Mesh EPIC #115 remains open (R&D). Prefer local CI runbook when GHA minutes are exhausted.
