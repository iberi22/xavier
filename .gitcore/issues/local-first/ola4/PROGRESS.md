# Ola 4 Progress Tracker

Base: main@2d6dc39c · cargo check OK · skill: jules-async-orchestration

| # | Issue | Task | Files | Jules | Status |
|---|------:|------|-------|-------|--------|
| 01 | 608 | Headless memory-fallback | headless_api.rs | YES | Triggered |
| 02 | 609 | Ollama models API | ollama_models.rs, server.rs | YES | Triggered |
| 03 | 610 | Panel UsageMetrics | panel-ui UsageMetricsPanel | YES | Triggered |
| 04 | 611 | Panel Ollama hot-swap UI | panel-ui OllamaModelManager | YES | Triggered (merge after 02) |
| 05 | 612 | E2E headless fallback | tests/e2e_chat_local.rs | YES | Triggered (merge after 01) |
| 06 | 613 | Docs USER_GUIDE | docs/ | YES | Triggered |
| 07 | 614 | EPIC close | features.json, ROADMAP, devlog | NO | Waiting |

## Merge order
1. #01, #02, #03 (parallel — different files)
2. #04 after #02
3. #05 after #01
4. #06 anytime
5. #07 last (add jules only if needed, or orchestrator)

## Anti-conflict ownership
- headless_api.rs -> only 01
- ollama_models + server routes -> only 02
- panel-ui metrics -> only 03
- panel-ui hotswap -> only 04
- tests/ -> only 05
- docs/USER_GUIDE -> only 06
- features.json -> only 07
