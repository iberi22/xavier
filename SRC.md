# SRC — Source Code Reference — xavier

> **Protocol:** GitCore 3.8.0  
> **Updated:** 2026-08-16
> **Completeness:** structure 100%

## 1. Overview

xavier — [![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)

| Field | Value |
|-------|--------|
| Path | `apps/xavier` |
| Stack | Rust (Tokio, Axum, SQLite, sqlite-vec), TypeScript (React, panel-ui) |
| Protocol | GitCore 3.8.0 |
| Visibility | private (SWAL default) |
| Pro model | SWAL node active (`pro_gate.rs`, no Stripe) |

## 2. Directory structure

```
xavier/
├── .git-core-protocol-version
├── .gitcore/
│   ├── AGENT_INDEX.md
│   ├── MANIFEST.json
│   ├── docs/
│   │   └── SWAL_GOAL.md
│   ├── features-detailed.json
│   └── features.json
├── AGENTS.md
├── CLA.md
├── CODE_OF_CONDUCT.md
├── CONTRIBUTING.md
├── Cargo.toml
├── LICENSE
├── README.md
├── SECURITY.md
├── SRC.md
├── code-graph/
│   ├── Cargo.lock
│   ├── Cargo.toml
│   ├── Dockerfile
│   ├── LICENSE
│   ├── MANUAL.md
│   ├── README.md
│   ├── SPEC.md
│   ├── examples/
│   │   └── index_xavier.rs
│   ├── fixtures/
│   │   ├── pipeline_test/
│   │   │   ├── c_test.c
│   │   │   ├── python_test.py
│   │   │   └── rust_test.rs
│   │   └── xavier-plugins/
│   │       ├── README.md
│   │       ├── plugins.json
│   │       └── plugins.schema.json
│   ├── parsers/
│   │   ├── parser-c/
│   │   │   ├── Cargo.toml
│   │   │   └── src/
│   │   ├── parser-cpp/
│   │   │   ├── Cargo.toml
│   │   │   └── src/
│   │   ├── parser-go/
│   │   │   ├── Cargo.toml
│   │   │   └── src/
│   │   ├── parser-java/
│   │   │   ├── Cargo.toml
│   │   │   └── src/
│   │   ├── parser-python/
│   │   │   ├── Cargo.toml
│   │   │   └── src/
│   │   ├── parser-rust/
│   │   │   ├── Cargo.toml
│   │   │   └── src/
│   │   └── parser-ts/
│   │       ├── Cargo.toml
│   │       └── src/
│   ├── src/
│   │   ├── api/
│   │   │   ├── mod.rs
│   │   │   └── plugin_routes.rs
│   │   ├── db/
│   │   │   ├── benchmarks.rs
│   │   │   └── mod.rs
│   │   ├── debug.rs
│   │   ├── error.rs
│   │   ├── indexer/
│   │   │   ├── call_resolution.rs
│   │   │   └── mod.rs
│   │   ├── lib.rs
│   │   ├── main.rs
│   │   ├── mcp.rs
│   │   ├── parser/
│   │   │   ├── c.rs
│   │   │   ├── cpp.rs
│   │   │   ├── go.rs
│   │   │   ├── java.rs
│   │   │   ├── mod.rs
│   │   │   ├── python.rs
│   │   │   ├── rust.rs
│   │   │   └── typescript.rs
│   │   ├── plugin/
│   │   │   ├── discovery.rs
│   │   │   ├── engine.rs
│   │   │   ├── fallback.rs
│   │   │   ├── health.rs
│   │   │   ├── manager.rs
│   │   │   ├── mod.rs
│   │   │   └── types.rs
│   │   ├── plugin_host.rs
│   │   ├── query/
│   │   │   ├── mod.rs
│   │   │   └── tests.rs
│   │   └── types.rs
│   ├── stormideas.md
│   └── tests/
│       ├── codegraph_test.rs
│       ├── multi_project_test.rs
│       └── pipeline_test.rs
├── docs/
│   ├── ARCHITECTURE/
│   │   ├── ARCHITECTURE.md
│   │   └── OVERVIEW.md
│   ├── CLI.md
│   ├── DEPLOY/
│   │   ├── CLOUD_RUN.md
│   │   ├── DEPLOYMENT.md
│   │   ├── DOCKER_DEPLOY.md
│   │   ├── README.md
│   │   └── SERVICE.md
│   ├── ENCRYPTION_SPEC.md
│   ├── FEATURE_STATUS.md
│   ├── KNOWN_VULNERABILITIES.md
│   ├── LOCAL_EMBEDDINGS.md
│   ├── LOCAL_SETUP.md
│   ├── MCP_CONTRACT.md
│   ├── MEMORY.md
│   ├── OPERATIONS.md
│   ├── PUBLIC_RELEASE_ROADMAP.md
│   ├── README.md
│   ├── ROADMAP.md
│   ├── SECURITY.md
│   ├── SRC/
│   │   ├── DATABASE.md
│   │   ├── GLOSSARY.md
│   │   ├── INTERFACES.md
│   │   ├── NON-FUNCTIONAL.md
│   │   ├── REQUIREMENTS.md
│   │   └── index.md
│   ├── SRS/
│   │   ├── ARCHITECTURE.md
│   │   ├── REQUIREMENTS.md
│   │   ├── USER-STORIES.md
│   │   └── index.md
│   ├── XAVIER_DATA_COMMONS_ARCHITECTURE.md
│   ├── XAVIER_DATA_COMMONS_FEATURES.md
│   ├── XAVIER_RAG_GUIDE.md
│   ├── adr/
│   │   ├── 001-memory-domain.md
│   │   ├── 002-ports-when.md
│   │   ├── 003-agent-state.md
│   │   ├── 004-cortex-plugin.md
│   │   ├── 005-multi-crate-migration.md
│   │   ├── 006-vector-store-local-sqlite-vec.md
│   │   ├── 007-codegraph-native-vs-colby.md
│   │   ├── 009-codegraph-maturity-bridge.md
│   │   ├── 016-canonical-data-dir.md
│   │   ├── ADR-013-compute-node-market-clavis-v2.md
│   │   ├── ADR-014-governance-dao-onchain.md
│   │   ├── ADR-015-node-provisioning.md
│   │   └── README.md
│   ├── advanced-settings.md
│   ├── api/
│   │   ├── README.md
│   │   ├── openapi.yaml
│   │   └── xavier.postman_collection.json
│   ├── archive/
│   │   ├── ADR-README.md
│   │   ├── AFTER_HORMER_PLAN.md
│   │   ├── AGENTS_GIT_CORE_PROTOCOL.md
│   │   ├── ALIGNMENT_AUDIT_2026-06-22.md
│   │   ├── ANTICIPATOR_ANALYSIS.md
│   │   ├── API-v0.10.md
│   │   ├── ARCHITECTURE-ADR-README.md
│   │   ├── ARCHITECTURE-root.md
│   │   ├── ARCHITECTURE_ANALYSIS.md
│   │   ├── BINCODE_MIGRATION.md
│   │   ├── CHANGELOG-2026-04-13.html
│   │   ├── CHANGELOG-MAY2026.md
│   │   ├── CHRONICLE_RAG_BLUEPRINT.md
│   │   ├── CLAUDECODE_TASK.md
│   │   ├── CLAUDE_GIT_CORE.md
│   │   ├── CLI_FIRST_RESEARCH.md
│   │   ├── CODE_REVIEW.md
│   │   ├── CODE_REVIEW_CODEX.md
│   │   ├── CODE_REVIEW_GEMINI.md
│   │   ├── CODE_REVIEW_MAY2026.md
│   │   ├── CODE_REVIEW_REPORT.md
│   │   ├── COMPETITIVE_ANALYSIS.md
│   │   ├── CORTEX.md
│   │   ├── CORTEX_NOTES.md
│   │   ├── CORTEX_QUICK_REFERENCE.md
│   │   ├── CORTEX_USAGE_GUIDE.md
│   │   ├── DEAD_CODE_SCAN.md
│   │   ├── DEPENDENCY_AUDIT_REPORT.md
│   │   ├── DEPENDENCY_STRATEGY.md
│   │   ├── FINE_TUNING_READINESS.md
│   │   ├── GOVERNANCE_DAO_PLAN.md
│   │   ├── GOVERNANCE_VISION.md
│   │   ├── GRAPHIFY_INTEGRATION.md
│   │   ├── GRAPH_LAYERS.md
│   │   ├── HEARTBEAT.md
│   │   ├── HORMER_IMPL_PLAN.md
│   │   ├── IDENTITY.md
│   │   ├── IMPROVEMENT_PLAN.md
│   │   ├── INFOGRAPHIC_SYSTEM.md
│   │   ├── INTERNAL_GOVERNANCE_DAO.md
│   │   ├── JULES_PROMPTS_MAY2026.md
│   │   ├── JULES_WORKFLOW.md
│   │   ├── LICENSE_MIGRATION.md
│   │   ├── LOCAL_LLM_BRIDGES.md
│   │   ├── MEMORY_IMPROVEMENT_PLAN.md
│   │   ├── MEMORY_MANAGER.md
│   │   ├── MESH_SYNC_PLAN.md
│   │   ├── MOBILE_GENERATIVE_UI.md
│   │   ├── MULTI_AGENT_PIPELINE.md
│   │   ├── MULTI_LAYER_MEMORY_SPEC.md
│   │   ├── OLA4_ANALYSIS.md
│   │   ├── PIPELINE.md
│   │   ├── PLAN.md
│   │   ├── PLAN_TRES_MEMORIAS.md
│   │   ├── PLAN_V1.md
│   │   ├── POLYGON_ANCHORS.md
│   │   ├── PRICING.md
│   │   ├── REFACTOR_PLAN.md
│   │   ├── REPO_RECONCILIATION_2026-05-08.md
│   │   ├── ROADMAP_AND_AUDIT.md
│   │   ├── ROADMAP_LOCAL_FIRST.md
│   │   ├── ROADMAP_v0.5.md
│   │   ├── SECURITY_DEPENDABOT.md
│   │   ├── SECURITY_LICENSE_SCAN.md
│   │   ├── STORAGE_SWITCH.md
│   │   ├── SWAL-ARCH.md
│   │   ├── TEST_COVERAGE_GAPS.md
│   │   ├── TODO.md
│   │   ├── TOKEN_SAVINGS_ANALYSIS.md
│   │   ├── TOOLS.md
│   │   ├── USER_GUIDE_LOCAL.md
│   │   ├── VALIDATION_PROMPTS.md
│   │   ├── VIDEO_SCRIPTS.md
│   │   ├── WHITEPAPER_SOVEREIGN_MESH.md
│   │   ├── WINDOWS_BUILD_INVESTIGATION.md
│   │   ├── XAVIER_RAG_MEMORY_AUDIT.md
│   │   ├── analysis/
│   │   ├── audit_report.md
│   │   ├── bug_report_OLD.md
│   │   ├── engram_backlog.md
│   │   ├── engram_extraction_analysis_prompt.md
│   │   ├── infographic.html
│   │   ├── issues/
│   │   ├── prompts/
│   │   ├── security/
│   │   ├── wiki/
│   │   └── xavier_cortex_engram_architecture.md
│   ├── assets/
│   │   ├── cortex_agents_interaction.png
│   │   ├── cortex_architecture_flow.png
│   │   └── ui/
│   ├── benchmark/
│   │   ├── BENCHMARK_COMPARISON.md
│   │   ├── BENCHMARK_PLAN.md
│   │   ├── DATA-MARKETPLACE.md
│   │   └── REPORT.md
│   ├── design/
│   │   ├── F12-PRESERVACION-MINI-EXPERTOS.md
│   │   └── F9-MESH-SWAL-PUBLICO-PRIVADO.md
│   ├── devlog/
│   │   ├── 2026-05-10-hexagonal-architecture.md
│   │   ├── 2026-05-10-prompt-injection-detector.md
│   │   ├── 2026-05-10-why-sqlite-vec.md
│   │   ├── 2026-05-29-local-first-cli-resilience-and-hexagonal-security.md
│   │   ├── 2026-07-02-sprint-completion.md
│   │   ├── 2026-07-17-ola3-usage-counters.md
│   │   ├── 2026-07-18-ola4-close.md
│   │   ├── 2026-07-31-ola10-ship-close.md
│   │   ├── 2026-07-31-ola11-harden-close.md
│   │   ├── INDEX.md
│   │   └── README.md
│   ├── explanation/
│   │   ├── DOCUMENTATION_MIGRATION.md
│   │   └── README.md
│   ├── features/
│   │   ├── README.md
│   │   ├── implementation-score.json
│   │   ├── messaging-integrations.md
│   │   └── specs/
│   ├── guides/
│   │   ├── CLI_REFERENCE.md
│   │   ├── CODEGRAPH_GIT_SYNC.md
│   │   ├── MCP_INTEGRATION.md
│   │   ├── QUICKSTART.md
│   │   ├── RAG_USAGE_GUIDE.md
│   │   └── WINDOWS_INSTALL.md
│   ├── how-to/
│   │   └── README.md
│   ├── legacy/
│   │   ├── biome.json
│   │   ├── features.json
│   │   └── patch.py
│   ├── ops/
│   │   ├── local-ci-with-agent-priv.md
│   │   ├── nixos-docker.md
│   │   ├── panel-build.md
│   │   ├── reindex-embeddings.md
│   │   └── release-packaging.md
│   ├── protocol/
│   │   ├── CLI_CONFIG.md
│   │   ├── PROTOCOL_REFERENCE.md
│   │   ├── README.md
│   │   ├── SDLC_WORKFLOW.md
│   │   └── rules/
│   ├── public/
│   │   ├── API.md
│   │   ├── ARCHITECTURE.md
│   │   ├── QUICKSTART.md
│   │   └── README.md
│   ├── reference/
│   │   ├── COMMIT_STANDARD.md
│   │   ├── CONFIG_REFERENCE.md
│   │   ├── ENV_VARS.md
│   │   └── README.md
│   ├── research/
│   │   ├── SELF-MANAGEMENT-RUNTIME.md
│   │   ├── embeddings-research.md
│   │   ├── ephemeral-key-proxy-report.md
│   │   └── xtsp/
│   ├── site/
│   │   ├── GITHUB_PAGES_PLAN.md
│   │   ├── README.md
│   │   ├── astro.config.mjs
│   │   ├── package.json
│   │   ├── public/
│   │   ├── src/
│   │   └── tsconfig.json
│   ├── source/
│   │   ├── a2a_SRC.md
│   │   ├── adapters_SRC.md
│   │   ├── agents_SRC.md
│   │   ├── api_SRC.md
│   │   ├── app_SRC.md
│   │   ├── auth2_SRC.md
│   │   ├── auto_improvement_SRC.md
│   │   ├── billing_SRC.md
│   │   ├── bin_SRC.md
│   │   ├── checkpoint_SRC.md
│   │   ├── chronicle_SRC.md
│   │   ├── cli_SRC.md
│   │   ├── codebase_SRC.md
│   │   ├── consistency_SRC.md
│   │   ├── consolidation_SRC.md
│   │   ├── context_SRC.md
│   │   ├── coordination_SRC.md
│   │   ├── crypto_SRC.md
│   │   ├── data_commons_SRC.md
│   │   ├── devlog_SRC.md
│   │   ├── domain_SRC.md
│   │   ├── embedding_SRC.md
│   │   ├── enterprise_SRC.md
│   │   ├── error_SRC.md
│   │   ├── governance_SRC.md
│   │   ├── health_SRC.md
│   │   ├── maturity_SRC.md
│   │   ├── memory_SRC.md
│   │   ├── mesh_SRC.md
│   │   ├── messaging_SRC.md
│   │   ├── middleware_SRC.md
│   │   ├── notifications_SRC.md
│   │   ├── observability_SRC.md
│   │   ├── ports_SRC.md
│   │   ├── retrieval_SRC.md
│   │   ├── scheduler_SRC.md
│   │   ├── search_SRC.md
│   │   ├── secrets_SRC.md
│   │   ├── security_SRC.md
│   │   ├── server_SRC.md
│   │   ├── session_SRC.md
│   │   ├── settings_SRC.md
│   │   ├── storage_SRC.md
│   │   ├── sync_SRC.md
│   │   ├── tasks_SRC.md
│   │   ├── telegram_SRC.md
│   │   ├── tgd_SRC.md
│   │   ├── time_SRC.md
│   │   ├── tools_SRC.md
│   │   ├── ui_SRC.md
│   │   ├── utils_SRC.md
│   │   ├── verification_SRC.md
│   │   └── workspace_SRC.md
│   ├── system/
│   │   ├── SPEC.md
│   │   ├── astro.config.mjs
│   │   ├── content/
│   │   ├── package.json
│   │   ├── src/
│   │   └── tsconfig.json
│   ├── templates/
│   │   ├── plan.md
│   │   ├── spec.md
│   │   └── tasks.md
│   ├── training_export_format.md
│   └── tutorials/
│       └── README.md
├── panel-ui/
│   ├── index.html
│   ├── package.json
│   ├── panel-ui
│   ├── playwright.config.ts
│   ├── pnpm-lock.yaml
│   ├── src/
│   │   ├── App.tsx
│   │   ├── api/
│   │   ├── auth/
│   │   ├── components/
│   │   ├── data.ts
│   │   ├── global.d.ts
│   │   ├── hooks/
│   │   ├── index.css
│   │   ├── main.tsx
│   │   ├── maloca/
│   │   ├── pages/
│   │   ├── store/
│   │   ├── types/
│   │   ├── types.ts
│   │   └── utils/
│   ├── src-tauri/
│   │   ├── Cargo.lock
│   │   ├── Cargo.toml
│   │   ├── app-icon.png
│   │   ├── binaries/
│   │   ├── build.rs
│   │   ├── capabilities/
│   │   ├── gen/
│   │   ├── icons/
│   │   ├── src/
│   │   └── tauri.conf.json
│   ├── tests/
│   │   ├── app.spec.ts
│   │   ├── auth.test.tsx
│   │   ├── chat.spec.ts
│   │   ├── dashboard.spec.ts
│   │   ├── generative-ui.spec.ts
│   │   ├── graphAdapters.test.ts
│   │   ├── inputArea.a11y.test.tsx
│   │   ├── mesh.spec.ts
│   │   ├── onboarding.spec.ts
│   │   ├── operationModeBadge.test.tsx
│   │   ├── roadmapGraph.test.ts
│   │   └── setup.ts
│   ├── tsconfig.generative.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   └── vitest.config.ts
├── parsers/
│   └── codegraph-parse-typescript/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           └── parser.rs
├── public/
│   ├── devlog/
│   │   ├── 2026-05-10-hexagonal-architecture.html
│   │   ├── 2026-05-10-prompt-injection-detector.html
│   │   ├── 2026-05-10-why-sqlite-vec.html
│   │   ├── commits-db.json
│   │   ├── index.html
│   │   ├── main.js
│   │   ├── review.html
│   │   └── style.css
│   └── maloca/
│       ├── 2026-05-10-hexagonal-architecture.html
│       ├── 2026-05-10-prompt-injection-detector.html
│       ├── 2026-05-10-why-sqlite-vec.html
│       ├── commits-db.json
│       ├── index.html
│       ├── main.js
│       ├── review.html
│       └── style.css
├── scripts/
│   ├── README.md
│   ├── _check_dbs.py
│   ├── _find_check_auth.py
│   ├── agent-sync-cron.sh
│   ├── agent-sync.ps1
│   ├── api-test.ps1
│   ├── backup.ps1
│   ├── benchmark_backends.py
│   ├── benchmark_cortex_engram.py
│   ├── benchmark_memory_systems.py
│   ├── benchmark_multi_cli.py
│   ├── benchmark_tri_memory.py
│   ├── benchmark_xavier_memory.cjs
│   ├── benchmark_xavier_memory.ps1
│   ├── benchmarks/
│   │   ├── bench_mini_experts.sh
│   │   ├── bench_real.js
│   │   ├── bench_real.py
│   │   ├── compare-benchmarks.js
│   │   ├── datasets/
│   │   ├── model_lab.example.json
│   │   ├── model_lab.py
│   │   ├── run_internal_memory_benchmark.py
│   │   ├── run_locomo_benchmark.py
│   │   ├── run_real_memory_benchmark.py
│   │   └── run_swebench_eval.py
│   ├── build-docker.sh
│   ├── build-local.ps1
│   ├── build-monitor.ps1
│   ├── build-tauri.ps1
│   ├── build-tauri.sh
│   ├── build-windows-installer.ps1
│   ├── build_synapse_dataset.py
│   ├── check-cortex.ps1
│   ├── check-env.ps1
│   ├── check-secrets.sh
│   ├── check-surreal.ps1
│   ├── check_alerts.ps1
│   ├── ci-local.sh
│   ├── cleanup_branches.ps1
│   ├── consolidate-stores.py
│   ├── cortex-backup-manager.js
│   ├── cortex-client.js
│   ├── cortex_cli.py
│   ├── create_issues.py
│   ├── curate_memory_data.py
│   ├── debug_alerts.ps1
│   ├── deploy-self-healing.ps1
│   ├── dismiss_alerts.ps1
│   ├── dismiss_alerts2.ps1
│   ├── eval/
│   │   ├── reports/
│   │   └── xavier_brain_eval.py
│   ├── export-session.ps1
│   ├── export_training_data.py
│   ├── extract_comprehensive_training.py
│   ├── feedback_usage_report.cjs
│   ├── feedback_usage_report.ps1
│   ├── final_benchmark.ps1
│   ├── fix_closing.py
│   ├── generate-devlog-post.sh
│   ├── generate-diff-db.js
│   ├── generate-starlight-docs.js
│   ├── health-watch.sh
│   ├── hooks/
│   │   ├── install-hooks.sh
│   │   ├── install-post-commit-codegraph.sh
│   │   ├── post-commit-codegraph.sh
│   │   └── pre-commit
│   ├── index-cli-histories.ps1
│   ├── index-conversations.js
│   ├── index-conversations.ps1
│   ├── index-production-data.ps1
│   ├── index-swal-code.js
│   ├── index_openclaw_sessions.ps1
│   ├── index_sessions.cjs
│   ├── install.ps1
│   ├── jules/
│   │   ├── check-api.js
│   │   ├── integrate-all.js
│   │   ├── jules-api.js
│   │   ├── list-sessions.js
│   │   └── retrigger-jules.js
│   ├── launch/
│   │   ├── launch_cortex.ps1
│   │   └── launch_cortex.sh
│   ├── list_issues.py
│   ├── locomo_benchmark.ps1
│   ├── locomo_benchmark_clean.ps1
│   ├── locomo_real_benchmark.ps1
│   ├── mcp/
│   │   └── xavier-mcp-cursor.sh
│   ├── measure_token_savings.py
│   ├── memory_triad_benchmark.py
│   ├── memory_triad_loop.ps1
│   ├── migrate-flattened-paths.py
│   ├── migrate_business_lines.ps1
│   ├── migrate_cortex.ps1
│   ├── migrate_file_to_sqlite.py
│   ├── migrate_file_to_surreal.py
│   ├── migrate_soul.ps1
│   ├── mini-expert-train.sh
│   ├── pre-commit-chronicle.sh
│   ├── pre-commit.sh
│   ├── project_status_agent.py
│   ├── project_status_agent_README.md
│   ├── rate-manager.js
│   ├── reconcile-xavier-clones.ps1
│   ├── reindex-embeddings.sh
│   ├── release-build.sh
│   ├── release-smoke.ps1
│   ├── release-smoke.sh
│   ├── run-benchmarks-docker.ps1
│   ├── run-embedding-benchmark.ps1
│   ├── run-sync-test.ps1
│   ├── run-sync.ps1
│   ├── run-task.ps1
│   ├── run_codex.ps1
│   ├── run_export.py
│   ├── run_extract.py
│   ├── run_gemini.ps1
│   ├── run_termux_docker_smoke.ps1
│   ├── ryzen-cppc-fix.py
│   ├── save_cycle.ps1
│   ├── send-alert.ps1
│   ├── setup-cortex-enterprise.sh
│   ├── setup-proxy-agent.bat
│   ├── setup-proxy-agent.sh
│   ├── smoke/
│   │   ├── README.md
│   │   └── mcp_contract.sh
│   ├── smoke_test_local.ps1
│   ├── smoke_test_local.sh
│   ├── stabilize-index.py
│   ├── stabilize-index.sh
│   ├── start-cortex.ps1
│   ├── start-xavier-windows.ps1
│   ├── start-xavier.ps1
│   ├── start.sh
│   ├── start_xavier_server.sh
│   ├── status-xavier.ps1
│   ├── stop-xavier.ps1
│   ├── subagents/
│   │   ├── dispatch.py
│   │   ├── generate_mcp_configs.py
│   │   ├── reports/
│   │   ├── run_ab_experiment.py
│   │   └── xavier_brain_prompt.md
│   ├── supabase-schema.sql
│   ├── super_benchmark.ps1
│   ├── super_benchmark_v2.ps1
│   ├── sync-all-to-cortex.js
│   ├── sync-operations-to-cortex.js
│   ├── sync_and_benchmark.py
│   ├── sync_run.ps1
│   ├── systemd/
│   │   ├── xavier-reindex.service
│   │   └── xavier-reindex.timer
│   ├── termux_docker_smoke.py
│   ├── test-cortex.ps1
│   ├── test-integration.ps1
│   ├── test-mcp.ps1
│   ├── test-persist.ps1
│   ├── test-sevier2-endpoints.ps1
│   ├── test-surrealdb.ps1
│   ├── test_agent.ps1
│   ├── test_agent_with_memory.ps1
│   ├── test_sevier2.ps1
│   ├── training/
│   │   └── train_lora_colab.py
│   ├── update-version.js
│   ├── validate-cron.js
│   ├── verify-pipeline.sh
│   ├── verify_xavier.js
│   ├── workflow.ps1
│   ├── xavier-benchmark-v2-result.json
│   ├── xavier-bpm-health.js
│   ├── xavier-bpm.bat
│   ├── xavier-brain.ps1
│   ├── xavier-claude-hook.sh
│   ├── xavier-full-test.ps1
│   ├── xavier-health-check.js
│   ├── xavier-indexer-v2.js
│   ├── xavier-optimizer.ps1
│   ├── xavier-service.ps1
│   ├── xavier.sh
│   └── xavier_client.ps1
├── src/
│   ├── a2a/
│   │   └── mod.rs
│   ├── adapters/
│   │   ├── inbound/
│   │   │   ├── http/
│   │   │   │   ├── dto.rs
│   │   │   │   ├── handlers/
│   │   │   │   │   ├── agent.rs
│   │   │   │   │   ├── ivn.rs
│   │   │   │   │   ├── marketplace.rs
│   │   │   │   │   ├── memory.rs
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── nodes.rs
│   │   │   │   │   ├── security.rs
│   │   │   │   │   └── sync.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── plugins/
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   └── pgheart.rs
│   │   │   │   ├── routes.rs
│   │   │   │   ├── state.rs
│   │   │   │   └── time_metrics_adapter.rs
│   │   │   └── mod.rs
│   │   ├── mod.rs
│   │   └── outbound/
│   │       ├── http_health_adapter.rs
│   │       └── mod.rs
│   ├── agents/
│   │   ├── anomaly_scanner.rs
│   │   ├── belief_evaluator.rs
│   │   ├── curation.rs
│   │   ├── cve_learner.rs
│   │   ├── evolve/
│   │   │   ├── config.rs
│   │   │   ├── evaluator.rs
│   │   │   ├── experiment.rs
│   │   │   ├── gap_analyzer.rs
│   │   │   ├── integrator.rs
│   │   │   ├── mod.rs
│   │   │   ├── mutator.rs
│   │   │   ├── reflector.rs
│   │   │   ├── researcher.rs
│   │   │   ├── results.rs
│   │   │   └── tests.rs
│   │   ├── extraction.rs
│   │   ├── hormer/
│   │   │   ├── mod.rs
│   │   │   ├── persistence_test.rs
│   │   │   ├── reward.rs
│   │   │   └── tests.rs
│   │   ├── mini_experts.rs
│   │   ├── mod.rs
│   │   ├── provider/
│   │   │   ├── anthropic.rs
│   │   │   ├── client.rs
│   │   │   ├── config.rs
│   │   │   ├── gemini.rs
│   │   │   ├── hardware.rs
│   │   │   ├── llama_cpp.rs
│   │   │   ├── local.rs
│   │   │   ├── minimax.rs
│   │   │   ├── mod.rs
│   │   │   ├── model_manager.rs
│   │   │   ├── openai.rs
│   │   │   ├── rate_limit.rs
│   │   │   ├── router.rs
│   │   │   ├── router_tests.rs
│   │   │   ├── tests.rs
│   │   │   ├── traits.rs
│   │   │   └── types.rs
│   │   ├── provider_router.rs
│   │   ├── rate_limit.rs
│   │   ├── registry.rs
│   │   ├── router.rs
│   │   ├── runtime.rs
│   │   ├── self_harness_coordinator.rs
│   │   ├── self_improve.rs
│   │   ├── supervisor.rs
│   │   ├── system1.rs
│   │   ├── system2.rs
│   │   ├── system3/
│   │   │   ├── client.rs
│   │   │   ├── engine.rs
│   │   │   ├── helpers/
│   │   │   │   ├── date.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── nlp.rs
│   │   │   │   └── text.rs
│   │   │   ├── mod.rs
│   │   │   ├── tests.rs
│   │   │   └── types.rs
│   │   ├── ui_render.rs
│   │   └── unregister_agent_handler.rs
│   ├── api/
│   │   ├── graph.rs
│   │   ├── mod.rs
│   │   ├── search.rs
│   │   ├── settings.rs
│   │   ├── skills.rs
│   │   └── timeline.rs
│   ├── app/
│   │   ├── health_service.rs
│   │   ├── memory_usecase.rs
│   │   ├── mod.rs
│   │   ├── proxy_use_case.rs
│   │   ├── proxy_use_case_tests.rs
│   │   ├── qmd_memory_adapter.rs
│   │   ├── security_service.rs
│   │   └── verification_service.rs
│   ├── auth2/
│   │   ├── db.rs
│   │   ├── jwt.rs
│   │   ├── middleware.rs
│   │   ├── mod.rs
│   │   ├── password.rs
│   │   └── refresh.rs
│   ├── auto_improvement/
│   │   ├── benchmark.rs
│   │   ├── cycle.rs
│   │   ├── experiments.rs
│   │   ├── gaps.rs
│   │   └── mod.rs
│   ├── billing/
│   │   ├── mod.rs
│   │   ├── plans.rs
│   │   ├── stripe_client.rs
│   │   └── webhook.rs
│   ├── bin/
│   │   ├── backfill_embeddings.rs
│   │   └── cortex.rs
│   ├── checkpoint/
│   │   ├── mod.rs
│   │   ├── session.rs
│   │   └── state.rs
│   ├── chronicle/
│   │   ├── auto_docs.rs
│   │   ├── cli.rs
│   │   ├── generate.rs
│   │   ├── harvest.rs
│   │   ├── mod.rs
│   │   ├── patterns.rs
│   │   ├── prompts.rs
│   │   ├── publish.rs
│   │   ├── redact.rs
│   │   └── ssg.rs
│   ├── clavis/
│   │   └── mod.rs
│   ├── cli/
│   │   ├── code_dump.rs
│   │   ├── codegraph_sync.rs
│   │   ├── commands/
│   │   │   ├── billing.rs
│   │   │   ├── cleanup.rs
│   │   │   ├── code.rs
│   │   │   ├── data_commons.rs
│   │   │   ├── enums.rs
│   │   │   ├── governance.rs
│   │   │   ├── http.rs
│   │   │   ├── improve.rs
│   │   │   ├── license.rs
│   │   │   ├── memory.rs
│   │   │   ├── mesh.rs
│   │   │   ├── mod.rs
│   │   │   ├── navigation.rs
│   │   │   ├── node.rs
│   │   │   ├── nodes.rs
│   │   │   ├── provider.rs
│   │   │   ├── regen.rs
│   │   │   ├── secrets.rs
│   │   │   ├── session.rs
│   │   │   ├── spawn.rs
│   │   │   ├── tasks.rs
│   │   │   ├── token.rs
│   │   │   ├── usage.rs
│   │   │   ├── verify.rs
│   │   │   └── wallet.rs
│   │   ├── config.rs
│   │   ├── handlers/
│   │   │   ├── agent.rs
│   │   │   ├── agent_cli.rs
│   │   │   ├── auth.rs
│   │   │   ├── billing.rs
│   │   │   ├── cloud.rs
│   │   │   ├── code.rs
│   │   │   ├── config.rs
│   │   │   ├── doctor.rs
│   │   │   ├── headless_api.rs
│   │   │   ├── headless_e2e.rs
│   │   │   ├── memory.rs
│   │   │   ├── mesh.rs
│   │   │   ├── mod.rs
│   │   │   ├── navigation.rs
│   │   │   ├── nodes.rs
│   │   │   ├── notifications.rs
│   │   │   ├── offline_models.rs
│   │   │   ├── ollama_models.rs
│   │   │   ├── onboarding.rs
│   │   │   ├── panel.rs
│   │   │   ├── plugins.rs
│   │   │   ├── provider.rs
│   │   │   ├── proxy_auth_tests.rs
│   │   │   ├── quota.rs
│   │   │   ├── recovery.rs
│   │   │   ├── secrets.rs
│   │   │   ├── security.rs
│   │   │   ├── setup.rs
│   │   │   ├── sync.rs
│   │   │   ├── system.rs
│   │   │   ├── system_scan.rs
│   │   │   ├── system_scan_cli.rs
│   │   │   ├── tasks.rs
│   │   │   ├── tokens.rs
│   │   │   ├── usage.rs
│   │   │   ├── verify.rs
│   │   │   ├── workspace.rs
│   │   │   └── workspace_db.rs
│   │   ├── http_setup.rs
│   │   ├── mcp.rs
│   │   ├── mod.rs
│   │   ├── onboarding.rs
│   │   ├── proxy.rs
│   │   ├── security.rs
│   │   ├── server.rs
│   │   ├── state.rs
│   │   ├── tests.rs
│   │   ├── types.rs
│   │   ├── utils.rs
│   │   └── websocket.rs
│   ├── codebase/
│   │   ├── codegraph_paths.rs
│   │   ├── codegraph_sidecar.rs
│   │   ├── connection_manager.rs
│   │   ├── conversations_db.rs
│   │   ├── db.rs
│   │   ├── issue_context.rs
│   │   ├── mod.rs
│   │   └── snapshot.rs
│   ├── consistency/
│   │   ├── mod.rs
│   │   └── regularization.rs
│   ├── consolidation/
│   │   ├── merger.rs
│   │   ├── mod.rs
│   │   └── reflection.rs
│   ├── context/
│   │   ├── bm25.rs
│   │   ├── builder.rs
│   │   ├── classifier.rs
│   │   ├── executor.rs
│   │   ├── graph_retriever.rs
│   │   ├── hybrid.rs
│   │   ├── indexer.rs
│   │   ├── manager.rs
│   │   ├── mod.rs
│   │   ├── monitoring.rs
│   │   ├── orchestrator.rs
│   │   ├── pipeline.rs
│   │   ├── query_processor.rs
│   │   ├── regen_loop.rs
│   │   ├── skill_dispatcher.rs
│   │   ├── skill_registry.rs
│   │   ├── skills.rs
│   │   ├── timeline.rs
│   │   └── token_estimate.rs
│   ├── coordination/
│   │   ├── agent_registry.rs
│   │   ├── agents.rs
│   │   ├── core.rs
│   │   ├── events.rs
│   │   ├── message_bus/
│   │   │   ├── agents.rs
│   │   │   ├── core.rs
│   │   │   ├── dlq.rs
│   │   │   ├── errors.rs
│   │   │   ├── handlers.rs
│   │   │   ├── metrics.rs
│   │   │   ├── mod.rs
│   │   │   ├── routing.rs
│   │   │   └── tests.rs
│   │   ├── mod.rs
│   │   └── secrets.rs
│   ├── crypto/
│   │   ├── encryption.rs
│   │   ├── hmac.rs
│   │   ├── keys.rs
│   │   ├── mod.rs
│   │   └── password.rs
│   ├── curation/
│   │   └── mod.rs
│   ├── data_commons/
│   │   ├── funnel.rs
│   │   ├── governance.rs
│   │   ├── ivn.rs
│   │   ├── maintainer.rs
│   │   ├── marketplace.rs
│   │   ├── mesh_bridge.rs
│   │   ├── mod.rs
│   │   ├── pricing.rs
│   │   ├── readiness.rs
│   │   ├── reputation.rs
│   │   ├── telemetry_db.rs
│   │   ├── training.rs
│   │   ├── types.rs
│   │   └── wallet.rs
│   ├── devlog/
│   │   ├── generator.rs
│   │   ├── mod.rs
│   │   └── models.rs
│   ├── domain/
│   │   ├── agent.rs
│   │   ├── audit.rs
│   │   ├── belief/
│   │   │   ├── mod.rs
│   │   │   └── types.rs
│   │   ├── error.rs
│   │   ├── memory/
│   │   │   ├── belief.rs
│   │   │   ├── graph.rs
│   │   │   └── mod.rs
│   │   ├── mod.rs
│   │   ├── pattern/
│   │   │   ├── mod.rs
│   │   │   └── types.rs
│   │   ├── proxy/
│   │   │   ├── mod.rs
│   │   │   └── types.rs
│   │   └── security/
│   │       ├── mod.rs
│   │       └── types.rs
│   ├── embedding/
│   │   ├── cache.rs
│   │   ├── gllm.rs
│   │   ├── mod.rs
│   │   ├── ollama.rs
│   │   ├── openai.rs
│   │   └── pipeline.rs
│   ├── enterprise/
│   │   ├── audit.rs
│   │   ├── http.rs
│   │   ├── keys.rs
│   │   ├── mod.rs
│   │   ├── persistence.rs
│   │   ├── rate_limit.rs
│   │   ├── rbac.rs
│   │   ├── tenant.rs
│   │   └── tests.rs
│   ├── error/
│   │   └── mod.rs
│   ├── governance/
│   │   ├── dao.rs
│   │   └── mod.rs
│   ├── health/
│   │   ├── history.rs
│   │   ├── mesh_telemetry.rs
│   │   ├── mod.rs
│   │   └── repair.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── main_tui.rs
│   ├── maloca/
│   │   ├── beliefs.rs
│   │   ├── commits.rs
│   │   ├── handlers.rs
│   │   ├── mod.rs
│   │   ├── params.rs
│   │   ├── store.rs
│   │   ├── timeline.rs
│   │   ├── types.rs
│   │   ├── universal.rs
│   │   └── ws.rs
│   ├── maturity/
│   │   ├── anchor.rs
│   │   ├── cli.rs
│   │   ├── mod.rs
│   │   ├── reporter.rs
│   │   ├── scanner/
│   │   │   ├── code_graph.rs
│   │   │   ├── conversations_scanner.rs
│   │   │   ├── memory_scanner.rs
│   │   │   ├── mod.rs
│   │   │   ├── old_types.rs
│   │   │   └── test_scanner.rs
│   │   ├── scorer.rs
│   │   └── tests.rs
│   ├── memory/
│   │   ├── README.md
│   │   ├── agent_indexer.rs
│   │   ├── agent_scanner.rs
│   │   ├── belief_graph.rs
│   │   ├── bridge.rs
│   │   ├── checkpoint_summary.rs
│   │   ├── cloud_sync.rs
│   │   ├── codex_importer.rs
│   │   ├── decay.rs
│   │   ├── embedder.rs
│   │   ├── entities.rs
│   │   ├── entity_graph/
│   │   │   ├── extraction.rs
│   │   │   ├── inference.rs
│   │   │   ├── mod.rs
│   │   │   ├── storage.rs
│   │   │   └── types.rs
│   │   ├── episodic.rs
│   │   ├── file_indexer.rs
│   │   ├── graph_store.rs
│   │   ├── graph_traversal.rs
│   │   ├── hermes_importer.rs
│   │   ├── hierarchy.rs
│   │   ├── jules_importer.rs
│   │   ├── languages.rs
│   │   ├── layers_config.rs
│   │   ├── manager/
│   │   │   ├── actions.rs
│   │   │   ├── compression.rs
│   │   │   ├── config.rs
│   │   │   ├── consolidation.rs
│   │   │   ├── core.rs
│   │   │   ├── decay.rs
│   │   │   ├── eviction.rs
│   │   │   ├── gc.rs
│   │   │   ├── management.rs
│   │   │   ├── mod.rs
│   │   │   ├── priority.rs
│   │   │   ├── quality.rs
│   │   │   ├── tests.rs
│   │   │   ├── tracking.rs
│   │   │   └── types.rs
│   │   ├── mod.rs
│   │   ├── openclaw_indexer.rs
│   │   ├── openclaw_scanner.rs
│   │   ├── pack.rs
│   │   ├── postgres_store.rs
│   │   ├── qmd/
│   │   │   ├── cache_warming.rs
│   │   │   ├── config.rs
│   │   │   ├── hash.rs
│   │   │   ├── mod.rs
│   │   │   ├── query_builder.rs
│   │   │   ├── reader.rs
│   │   │   ├── search/
│   │   │   │   ├── embedding.rs
│   │   │   │   ├── hybrid.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── resolution.rs
│   │   │   │   ├── scoring.rs
│   │   │   │   ├── tests.rs
│   │   │   │   └── vector.rs
│   │   │   ├── types.rs
│   │   │   ├── utils.rs
│   │   │   └── writer.rs
│   │   ├── qmd_memory.rs
│   │   ├── schema.rs
│   │   ├── semantic.rs
│   │   ├── semantic_cache.rs
│   │   ├── simple_index.rs
│   │   ├── snippet.rs
│   │   ├── sqlite_store.rs
│   │   ├── sqlite_vec_store/
│   │   │   ├── audit.rs
│   │   │   ├── backend_impl.rs
│   │   │   ├── config.rs
│   │   │   ├── db.rs
│   │   │   ├── fts.rs
│   │   │   ├── graph.rs
│   │   │   ├── mod.rs
│   │   │   ├── schema_impl.rs
│   │   │   ├── search.rs
│   │   │   ├── store_impl.rs
│   │   │   ├── types.rs
│   │   │   ├── utils.rs
│   │   │   └── vector.rs
│   │   ├── store.rs
│   │   ├── supabase_store.rs
│   │   ├── sync/
│   │   │   ├── diff.rs
│   │   │   ├── manifest.rs
│   │   │   ├── merge.rs
│   │   │   ├── mod.rs
│   │   │   └── push_pull.rs
│   │   ├── telemetry.rs
│   │   ├── tests.rs
│   │   ├── virtual_memory.rs
│   │   └── working.rs
│   ├── mesh/
│   │   ├── acl.rs
│   │   ├── auth.rs
│   │   ├── auto_update.rs
│   │   ├── challenge.rs
│   │   ├── cloud_node.rs
│   │   ├── context_bridge.rs
│   │   ├── crypto_gating.rs
│   │   ├── dashboard.rs
│   │   ├── data_consent.rs
│   │   ├── data_sanitizer.rs
│   │   ├── governance/
│   │   │   ├── mod.rs
│   │   │   └── onchain.rs
│   │   ├── heartbeat.rs
│   │   ├── iroh_transport.rs
│   │   ├── maturity.rs
│   │   ├── mod.rs
│   │   ├── namespace.rs
│   │   ├── node.rs
│   │   ├── pairing.rs
│   │   ├── pairing_registry.rs
│   │   ├── peer.rs
│   │   ├── private_mesh.rs
│   │   ├── pro_gate.rs
│   │   ├── protocol.rs
│   │   ├── public_directory.rs
│   │   ├── public_rag.rs
│   │   ├── registry.rs
│   │   ├── service_network.rs
│   │   ├── telemetry.rs
│   │   ├── telemetry_collector.rs
│   │   ├── tokenomics/
│   │   │   ├── accounting.rs
│   │   │   ├── contracts.rs
│   │   │   ├── economy.rs
│   │   │   ├── mod.rs
│   │   │   ├── rewards.rs
│   │   │   ├── tests.rs
│   │   │   ├── vesting.rs
│   │   │   └── wallet.rs
│   │   └── transport/
│   │       └── mod.rs
│   ├── messaging/
│   │   ├── discord.rs
│   │   └── mod.rs
│   ├── middleware/
│   │   ├── auth.rs
│   │   ├── mod.rs
│   │   └── token_bucket.rs
│   ├── models/
│   │   ├── mini_expert.rs
│   │   └── mod.rs
│   ├── node_identity/
│   │   ├── bip39_seed.rs
│   │   ├── check_codes.rs
│   │   ├── derive.rs
│   │   ├── hybrid_pack.rs
│   │   ├── mod.rs
│   │   ├── persist.rs
│   │   ├── shamir.rs
│   │   └── vault.rs
│   ├── nodes/
│   │   ├── audit.rs
│   │   ├── cert.rs
│   │   ├── mod.rs
│   │   ├── provision.rs
│   │   ├── registry.rs
│   │   └── secrets.rs
│   ├── notifications/
│   │   ├── dispatcher.rs
│   │   └── mod.rs
│   ├── observability/
│   │   ├── README.md
│   │   ├── analyzer.rs
│   │   ├── detector.rs
│   │   ├── fixer.rs
│   │   ├── health.rs
│   │   ├── middleware.rs
│   │   ├── mod.rs
│   │   ├── notifier.rs
│   │   ├── service_log.rs
│   │   ├── token_accounting.rs
│   │   └── usage_counters.rs
│   ├── plugins/
│   │   ├── mod.rs
│   │   └── runtime.rs
│   ├── polygon_anchor/
│   │   ├── abi.rs
│   │   ├── broadcast.rs
│   │   └── mod.rs
│   ├── ports/
│   │   ├── inbound/
│   │   │   ├── agent_lifecycle_port.rs
│   │   │   ├── health_port.rs
│   │   │   ├── input_security_port.rs
│   │   │   ├── memory_port.rs
│   │   │   ├── mod.rs
│   │   │   ├── security_port.rs
│   │   │   ├── session_port.rs
│   │   │   ├── session_sync_port.rs
│   │   │   ├── time_metrics_port.rs
│   │   │   └── verification_port.rs
│   │   ├── mod.rs
│   │   └── outbound/
│   │       ├── embedding_port.rs
│   │       ├── health_check_port.rs
│   │       ├── mod.rs
│   │       ├── schema_init.rs
│   │       └── threat_detection_port.rs
│   ├── retrieval/
│   │   ├── config.rs
│   │   ├── eval.rs
│   │   ├── gating.rs
│   │   ├── history.rs
│   │   ├── mod.rs
│   │   ├── navigation.rs
│   │   ├── policy.rs
│   │   ├── scoring.rs
│   │   └── tuner.rs
│   ├── scheduler/
│   │   ├── daemon.rs
│   │   ├── job.rs
│   │   ├── mod.rs
│   │   ├── retry.rs
│   │   └── retry_tests.rs
│   ├── search/
│   │   ├── bm25.rs
│   │   ├── hooks.rs
│   │   ├── hybrid.rs
│   │   ├── mod.rs
│   │   ├── rerank.rs
│   │   └── rrf.rs
│   ├── secrets/
│   │   ├── audit.rs
│   │   ├── lending.rs
│   │   ├── local.rs
│   │   ├── local_vault.rs
│   │   ├── mod.rs
│   │   ├── openbao.rs
│   │   ├── store.rs
│   │   ├── tests.rs
│   │   └── vault.rs
│   ├── security/
│   │   ├── acl/
│   │   │   ├── hierarchy.rs
│   │   │   └── mod.rs
│   │   ├── anticipator.rs
│   │   ├── audit.rs
│   │   ├── auth.rs
│   │   ├── auth_store.rs
│   │   ├── clearance.rs
│   │   ├── detections.rs
│   │   ├── encryption_keys.rs
│   │   ├── groups.rs
│   │   ├── initializer.rs
│   │   ├── layers/
│   │   │   ├── canary.rs
│   │   │   ├── config_drift.rs
│   │   │   ├── encoding.rs
│   │   │   ├── entropy.rs
│   │   │   ├── goal_drift.rs
│   │   │   ├── heuristic.rs
│   │   │   ├── homoglyph.rs
│   │   │   ├── mod.rs
│   │   │   ├── path_traversal.rs
│   │   │   ├── phrase.rs
│   │   │   ├── threat_categories.rs
│   │   │   └── tool_alias.rs
│   │   ├── license.rs
│   │   ├── mod.rs
│   │   ├── prompt_guard.rs
│   │   ├── recovery.rs
│   │   ├── redaction.rs
│   │   ├── rsa_keys.rs
│   │   ├── scanner/
│   │   │   ├── entropy.rs
│   │   │   ├── mod.rs
│   │   │   ├── phrase_matcher.rs
│   │   │   └── scanner_impl.rs
│   │   ├── sessions.rs
│   │   ├── threat_store.rs
│   │   ├── tokens.rs
│   │   ├── url_validator.rs
│   │   └── user_store.rs
│   ├── self_manage/
│   │   └── mod.rs
│   ├── server/
│   │   ├── alerts.rs
│   │   ├── events.rs
│   │   ├── f12_routes.rs
│   │   ├── headless/
│   │   │   ├── auth.rs
│   │   │   ├── mod.rs
│   │   │   └── routes.rs
│   │   ├── http/
│   │   │   ├── api.rs
│   │   │   ├── context.rs
│   │   │   ├── health.rs
│   │   │   ├── mod.rs
│   │   │   ├── types.rs
│   │   │   ├── v1.rs
│   │   │   └── websocket.rs
│   │   ├── mcp/
│   │   │   ├── auth.rs
│   │   │   ├── mod.rs
│   │   │   ├── progressive.rs
│   │   │   ├── regression_token_savings.rs
│   │   │   ├── server.rs
│   │   │   ├── session.rs
│   │   │   ├── tests.rs
│   │   │   ├── tools_context.rs
│   │   │   ├── tools_core.rs
│   │   │   ├── tools_memory.rs
│   │   │   ├── transport.rs
│   │   │   └── types.rs
│   │   ├── mcp_stdio.rs
│   │   ├── mod.rs
│   │   ├── panel/
│   │   │   ├── assets.rs
│   │   │   ├── chat.rs
│   │   │   ├── mod.rs
│   │   │   ├── storage.rs
│   │   │   ├── threads.rs
│   │   │   └── types.rs
│   │   ├── training_routes.rs
│   │   └── v1_api.rs
│   ├── session/
│   │   ├── auto_save.rs
│   │   ├── event_mapper.rs
│   │   ├── indexer.rs
│   │   ├── integration_test.rs
│   │   ├── mod.rs
│   │   ├── sharing.rs
│   │   └── types.rs
│   ├── settings/
│   │   ├── defaults.rs
│   │   ├── env.rs
│   │   ├── mod.rs
│   │   ├── serialization.rs
│   │   ├── types.rs
│   │   └── validation.rs
│   ├── storage/
│   │   ├── migrations.rs
│   │   ├── mod.rs
│   │   └── multi_db.rs
│   ├── sync/
│   │   ├── chunks.rs
│   │   ├── manifest.rs
│   │   ├── mod.rs
│   │   └── transport.rs
│   ├── tasks/
│   │   ├── mod.rs
│   │   ├── models.rs
│   │   ├── scoring.rs
│   │   ├── session_sync_task.rs
│   │   ├── store.rs
│   │   └── sync.rs
│   ├── telegram/
│   │   └── mod.rs
│   ├── tgd/
│   │   ├── cache.rs
│   │   ├── consolidation.rs
│   │   └── mod.rs
│   ├── time/
│   │   └── mod.rs
│   ├── tools/
│   │   ├── gitcore_tools.rs
│   │   ├── kanban.rs
│   │   ├── mod.rs
│   │   ├── search_tools.rs
│   │   └── validation_tools.rs
│   ├── ui/
│   │   ├── dashboard.rs
│   │   ├── log_stream.rs
│   │   ├── memory_view.rs
│   │   └── mod.rs
│   ├── utils/
│   │   ├── compression.rs
│   │   ├── crypto.rs
│   │   ├── file_traversal.rs
│   │   ├── http.rs
│   │   ├── mod.rs
│   │   └── tauri_utils.rs
│   ├── verification/
│   │   ├── auto_verifier.rs
│   │   ├── cycle.rs
│   │   ├── feature_scanner.rs
│   │   └── mod.rs
│   └── workspace/
│       ├── config.rs
│       ├── mod.rs
│       ├── ops.rs
│       ├── registry.rs
│       ├── state.rs
│       ├── templates.rs
│       ├── tests.rs
│       └── usage.rs
├── tests/
│   ├── agent_lease_hooks_tests.rs
│   ├── agent_task_lifecycle_tests.rs
│   ├── benchmark.rs
│   ├── chronicle_harvest_test.rs
│   ├── clavis_integration.rs
│   ├── data_commons.rs
│   ├── e2e/
│   │   ├── clearance_access_audit.rs
│   │   ├── context_regen_e2e.rs
│   │   ├── crypto_e2e.rs
│   │   ├── decentralized_login_e2e.rs
│   │   ├── governance_e2e.rs
│   │   ├── hormer_navigation_e2e.rs
│   │   ├── mcp_e2e.rs
│   │   ├── multi_node_sync.rs
│   │   ├── notification_e2e.rs
│   │   ├── rbac_e2e.rs
│   │   └── telegram_bot_e2e.rs
│   ├── e2e_chat_local.rs
│   ├── e2e_system_alerts.ps1
│   ├── embedding_fallback_test.rs
│   ├── embedding_local_integration.rs
│   ├── error_test.rs
│   ├── fixtures/
│   │   └── headless_config.yaml
│   ├── headless_api_test.rs
│   ├── headless_chat_test.rs
│   ├── health_test.rs
│   ├── hormer_e2e.rs
│   ├── hormer_integration.rs
│   ├── hormer_scale.rs
│   ├── integration/
│   │   ├── a2a_test.rs
│   │   ├── agents_test.rs
│   │   ├── auth_full_flow_test.rs
│   │   ├── auth_register_test.rs
│   │   ├── belief_graph_test.rs
│   │   ├── checkpoint_test.rs
│   │   ├── cli.rs
│   │   ├── cli_test.rs
│   │   ├── codegraph_dump_test.rs
│   │   ├── context_regen_test.rs
│   │   ├── coordination_test.rs
│   │   ├── governance_integration_test.rs
│   │   ├── hierarchical_curation_test.rs
│   │   ├── http_api.rs
│   │   ├── internal_benchmark_test.rs
│   │   ├── issue_context_test.rs
│   │   ├── memory_test.rs
│   │   ├── notifications_test.rs
│   │   ├── scheduler_test.rs
│   │   ├── security_hardening_test.rs
│   │   ├── security_test.rs
│   │   ├── server_test.rs
│   │   ├── tasks_test.rs
│   │   └── test_common.rs
│   ├── integration.rs
│   ├── ivn_api.rs
│   ├── ivn_karma.rs
│   ├── ivn_verdict.rs
│   ├── marketplace_api.rs
│   ├── memory_prune_test.rs
│   ├── memory_sync_e2e.rs
│   ├── memory_sync_http.rs
│   ├── mesh_integration.rs
│   ├── mesh_iroh_test.rs
│   ├── mesh_management_test.rs
│   ├── mesh_node_lifecycle_test.rs
│   ├── mesh_peer_registry_test.rs
│   ├── mesh_permissions_test.rs
│   ├── mesh_security_sync_test.rs
│   ├── mini_expert_integration_test.rs
│   ├── node_fase0_persist.rs
│   ├── nodes_provisioning_test.rs
│   ├── proxy_integration.rs
│   ├── proxy_lending_integration.rs
│   ├── proxy_security_test.rs
│   ├── quick_embed_test.rs
│   ├── rate_limit_integration.rs
│   ├── recovery_flow.rs
│   ├── reindex_script_test.sh
│   ├── secrets_redaction_test.rs
│   ├── security_redact.rs
│   ├── semantic_search_tests.rs
│   ├── server_e2e.rs
│   ├── sevier_stress_test.rs
│   ├── sqlite_vec_validation.rs
│   ├── storage_isolation.rs
│   ├── stress_tests.rs
│   ├── swal_benchmarks.rs
│   ├── sync_check_handler_cached_result.rs
│   ├── sync_test.rs
│   ├── tgd_consolidation_test.rs
│   ├── tui_screenshot_e2e.rs
│   ├── unified_storage_validation.rs
│   ├── v1_memories_add_path_traversal.rs
│   ├── websocket_events.rs
│   ├── workspace_federation_test.rs
│   └── xtsp/
│       ├── mod.rs
│       └── validation.rs
└── xavier-core/
    ├── Cargo.toml
    ├── README.md
    └── src/
        ├── checkpoint/
        │   ├── mod.rs
        │   ├── session.rs
        │   └── state.rs
        ├── codebase/
        │   ├── connection_manager.rs
        │   └── mod.rs
        ├── config.rs
        ├── crypto/
        │   ├── encryption.rs
        │   ├── keys.rs
        │   └── mod.rs
        ├── domain/
        │   ├── memory/
        │   └── mod.rs
        ├── embedding/
        │   ├── cache.rs
        │   ├── gllm.rs
        │   ├── mod.rs
        │   ├── openai.rs
        │   └── pipeline.rs
        ├── hybrid.rs
        ├── lib.rs
        ├── memory/
        │   ├── mod.rs
        │   └── qmd/
        ├── memory_hierarchy.rs
        ├── rerank.rs
        ├── retrieval/
        │   ├── config.rs
        │   └── mod.rs
        ├── rrf.rs
        ├── schema.rs
        ├── search_hooks.rs
        ├── security/
        │   └── mod.rs
        ├── server/
        │   ├── events.rs
        │   └── mod.rs
        ├── settings/
        │   ├── defaults.rs
        │   ├── env.rs
        │   ├── mod.rs
        │   ├── serialization.rs
        │   ├── types.rs
        │   └── validation.rs
        ├── sqlite_vec_store/
        │   ├── audit.rs
        │   ├── backend_impl.rs
        │   ├── config.rs
        │   ├── db.rs
        │   ├── fts.rs
        │   ├── graph.rs
        │   ├── mod.rs
        │   ├── schema_impl.rs
        │   ├── search.rs
        │   ├── store_impl.rs
        │   ├── types.rs
        │   ├── utils.rs
        │   └── vector.rs
        ├── store.rs
        └── utils/
            ├── crypto.rs
            ├── hardware_vault_stub.rs
            └── mod.rs

```

| Component | Path | Purpose |
|-----------|------|---------|
| Protocol meta | `.gitcore/` | Architecture, features, planning, SWAL GOAL |
| SRS | `docs/SRS/` | Formal IEEE-830 requirements |
| Agent rules | `AGENTS.md` | Read order, conventions, SWAL integration block |
| Domain logic | `src/` | Hexagonal core, memory, mesh, HTTP/MCP servers |
| Core crate | `xavier-core/` | Low-level vector storage & embedding primitives |
| Code graph | `code-graph/` | AST indexing sidecar & symbol query engine |
| Web UI | `panel-ui/` | React dashboard and Maloca portal components |

## 4. Build / run / test

```bash
# Build binary
cargo build --release

# Run unit and integration tests
cargo test --workspace

# Lint code
cargo clippy --all-targets -- -D warnings

# Verify local audit
python3 /home/jules/self_created_tools/swal_docs_audit_local.py
```

## 5. Environment

| Variable | Purpose | Required |
|----------|---------|----------|
| `XAVIER_URL` | Xavier HTTP (default http://127.0.0.1:8006) | for agentic memory |
| `XAVIER_TOKEN` | Auth token | when server enforces auth |
| `XAVIER_DATA_DIR` | Vault + identity + anchor receipts | node identity |
| `XAVIER_NODE_DEVICE_KEY` | Device key for node auth | optional |
| `SWAL_POLYGON_RPC_URL` | RPC Polygon (Amoy/mainnet) | anchors live |
| `SWAL_ANCHOR_CONTRACT` | Registry address post-deploy | anchors live |
| `SWAL_ANCHOR_KEY` | Signer key | anchors live |
| `SWAL_ANCHOR_DRY_RUN` | `1` default — no broadcast | anchors |
| `SWAL_ANCHOR_BROADCAST` | `1` + `--features dao-evm` → live tx | anchors |

Never commit real secrets.

## 6. SWAL integration

| Concern | Approach |
|---------|----------|
| Pro features | Active SWAL node (`pro_gate.rs` + heartbeat) |
| Multi-instance | `instance_id` · namespaces `swal/{app}/{instance}` |
| Memory | Xavier HTTP/MCP |
| Mesh | Xavier Mesh P2P / Iroh transport |
| Identity / login | `src/node_identity/` + `src/polygon_anchor/` |
| Payments Pro | **No Stripe** |

## 7. Cross-references

| Doc | Path |
|-----|------|
| AGENTS.md | `AGENTS.md` |
| SWAL GOAL | `.gitcore/docs/SWAL_GOAL.md` |
| features | `.gitcore/features.json` |
| SRS index | `docs/SRS/index.md` |
| SRS REQ-008 | `docs/SRS/REQUIREMENTS.md` |
| CLA | `CLA.md` |

---

*Document version: 3.8.0 · Part of GitCore Protocol · Updated 2026-08-16*
