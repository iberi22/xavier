# Ola 11 — File Ownership Map

Updated: 2026-07-31

| Issue | Files (ONLY) | Parallel? |
|-------|--------------|-----------|
| 01 headless code_* | `src/server/headless/routes.rs` | ✅ |
| 02 offline_models test | `src/cli/handlers/offline_models.rs` | ✅ |
| 03 health prioritization | `src/health/mod.rs` | ✅ |
| 04 reindex embeddings | `src/memory/sqlite_vec_store/schema_impl.rs` | ✅ |
| 05 dedup PathExact | `src/memory/sqlite_vec_store/store_impl.rs` (+ `src/memory/tests.rs` if threshold fixture must change) | ✅ vs 04 |
| 06 settings local-first | `config/xavier.config.json`, `src/settings/defaults.rs` | ✅ |
| 07 panel 503 UX | `src/server/panel/assets.rs` | ✅ |
| 08 onboarding auth truth | `panel-ui/src/components/Onboarding/OnboardingFlow.tsx`, `panel-ui/src/components/Onboarding/AuthStep.tsx` | ✅ |
| 09 NixOS Docker ops doc | `docs/ops/nixos-docker.md` (NEW) | ✅ docs |
| 10 WiX residual | `installer/xavier.wxs`, `docs/FEATURE_STATUS.md` | ✅ |
| 11 local CI ops doc | `docs/ops/local-ci-with-agent-priv.md` (NEW) | ✅ docs |
| 12 EPIC close | `.gitcore/features.json`, `.gitcore/features-detailed.json`, `docs/devlog/2026-07-31-ola11-harden-close.md` | 🔒 LAST |

**Conflicts:** none if 05 does not edit `schema_impl.rs`.  
**features.json:** only issue 12.
