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
├── AGENTS.md
├── CLA.md
├── CONTRIBUTING.md
├── LICENSE
├── README.md
├── SECURITY.md
├── SRC.md
├── Cargo.toml
├── .git-core-protocol-version
├── .gitcore/
│   ├── AGENT_INDEX.md
│   ├── MANIFEST.json
│   ├── docs/
│   │   └── SWAL_GOAL.md
│   ├── features.json
│   └── features-detailed.json
├── docs/
│   ├── ARCHITECTURE/
│   ├── CLI.md
│   ├── OPERATIONS.md
│   ├── ROADMAP.md
│   ├── SECURITY.md
│   └── SRS/
│       ├── index.md
│       ├── REQUIREMENTS.md
│       └── ARCHITECTURE.md
├── src/
│   ├── a2a
│   │   └── mod.rs
│   ├── adapters
│   │   ├── inbound
│   │   │   ├── http
│   │   │   │   ├── dto.rs
│   │   │   │   ├── handlers
│   │   │   │   │   ├── agent.rs
│   │   │   │   │   ├── ivn.rs
│   │   │   │   │   ├── marketplace.rs
│   │   │   │   │   ├── memory.rs
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── nodes.rs
│   │   │   │   │   ├── security.rs
│   │   │   │   │   └── sync.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── plugins
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   └── pgheart.rs
│   │   │   │   ├── routes.rs
│   │   │   │   ├── state.rs
│   │   │   │   └── time_metrics_adapter.rs
│   │   │   └── mod.rs
│   │   ├── mod.rs
│   │   └── outbound
│   │       ├── http_health_adapter.rs
│   │       └── mod.rs
│   ├── agents
│   │   ├── anomaly_scanner.rs
│   │   ├── belief_evaluator.rs
│   │   ├── curation.rs
│   │   ├── cve_learner.rs
│   │   ├── evolve
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
│   │   ├── hormer
│   │   │   ├── mod.rs
│   │   │   ├── persistence_test.rs
│   │   │   ├── reward.rs
│   │   │   └── tests.rs
│   │   ├── mini_experts.rs
│   │   ├── mod.rs
│   │   ├── provider
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
│   │   ├── system3
│   │   │   ├── client.rs
│   │   │   ├── engine.rs
│   │   │   ├── helpers
│   │   │   │   ├── date.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── nlp.rs
│   │   │   │   └── text.rs
│   │   │   ├── mod.rs
│   │   │   ├── tests.rs
│   │   │   └── types.rs
│   │   ├── ui_render.rs
│   │   └── unregister_agent_handler.rs
│   ├── api
│   │   ├── graph.rs
│   │   ├── mod.rs
│   │   ├── search.rs
│   │   ├── settings.rs
│   │   ├── skills.rs
│   │   └── timeline.rs
│   ├── app
│   │   ├── health_service.rs
│   │   ├── memory_usecase.rs
│   │   ├── mod.rs
│   │   ├── proxy_use_case.rs
│   │   ├── proxy_use_case_tests.rs
│   │   ├── qmd_memory_adapter.rs
│   │   ├── security_service.rs
│   │   └── verification_service.rs
│   ├── auth2
│   │   ├── db.rs
│   │   ├── jwt.rs
│   │   ├── middleware.rs
│   │   ├── mod.rs
│   │   ├── password.rs
│   │   └── refresh.rs
│   ├── auto_improvement
│   │   ├── benchmark.rs
│   │   ├── cycle.rs
│   │   ├── experiments.rs
│   │   ├── gaps.rs
│   │   └── mod.rs
│   ├── billing
│   │   ├── mod.rs
│   │   ├── plans.rs
│   │   ├── stripe_client.rs
│   │   └── webhook.rs
│   ├── bin
│   │   ├── backfill_embeddings.rs
│   │   └── cortex.rs
│   ├── checkpoint
│   │   ├── mod.rs
│   │   ├── session.rs
│   │   └── state.rs
│   ├── chronicle
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
│   ├── clavis
│   │   └── mod.rs
│   ├── cli
│   │   ├── code_dump.rs
│   │   ├── codegraph_sync.rs
│   │   ├── commands
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
│   │   ├── handlers
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
│   ├── codebase
│   │   ├── codegraph_paths.rs
│   │   ├── codegraph_sidecar.rs
│   │   ├── connection_manager.rs
│   │   ├── conversations_db.rs
│   │   ├── db.rs
│   │   ├── issue_context.rs
│   │   ├── mod.rs
│   │   └── snapshot.rs
│   ├── consistency
│   │   ├── mod.rs
│   │   └── regularization.rs
│   ├── consolidation
│   │   ├── merger.rs
│   │   ├── mod.rs
│   │   └── reflection.rs
│   ├── context
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
│   ├── coordination
│   │   ├── agent_registry.rs
│   │   ├── agents.rs
│   │   ├── core.rs
│   │   ├── events.rs
│   │   ├── message_bus
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
│   ├── crypto
│   │   ├── encryption.rs
│   │   ├── hmac.rs
│   │   ├── keys.rs
│   │   ├── mod.rs
│   │   └── password.rs
│   ├── curation
│   │   └── mod.rs
│   ├── data_commons
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
│   ├── devlog
│   │   ├── generator.rs
│   │   ├── mod.rs
│   │   └── models.rs
│   ├── domain
│   │   ├── agent.rs
│   │   ├── audit.rs
│   │   ├── belief
│   │   │   ├── mod.rs
│   │   │   └── types.rs
│   │   ├── error.rs
│   │   ├── memory
│   │   │   ├── belief.rs
│   │   │   ├── graph.rs
│   │   │   └── mod.rs
│   │   ├── mod.rs
│   │   ├── pattern
│   │   │   ├── mod.rs
│   │   │   └── types.rs
│   │   ├── proxy
│   │   │   ├── mod.rs
│   │   │   └── types.rs
│   │   └── security
│   │       ├── mod.rs
│   │       └── types.rs
│   ├── embedding
│   │   ├── cache.rs
│   │   ├── gllm.rs
│   │   ├── mod.rs
│   │   ├── ollama.rs
│   │   ├── openai.rs
│   │   └── pipeline.rs
│   ├── enterprise
│   │   ├── audit.rs
│   │   ├── http.rs
│   │   ├── keys.rs
│   │   ├── mod.rs
│   │   ├── persistence.rs
│   │   ├── rate_limit.rs
│   │   ├── rbac.rs
│   │   ├── tenant.rs
│   │   └── tests.rs
│   ├── error
│   │   └── mod.rs
│   ├── governance
│   │   ├── dao.rs
│   │   └── mod.rs
│   ├── health
│   │   ├── history.rs
│   │   ├── mesh_telemetry.rs
│   │   ├── mod.rs
│   │   └── repair.rs
│   ├── humanchallenge
│   │   ├── cron.rs
│   │   ├── mod.rs
│   │   ├── scanner.rs
│   │   ├── store.rs
│   │   └── types.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── main_tui.rs
│   ├── maloca
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
│   ├── maturity
│   │   ├── anchor.rs
│   │   ├── cli.rs
│   │   ├── mod.rs
│   │   ├── reporter.rs
│   │   ├── scanner
│   │   │   ├── code_graph.rs
│   │   │   ├── conversations_scanner.rs
│   │   │   ├── memory_scanner.rs
│   │   │   ├── mod.rs
│   │   │   ├── old_types.rs
│   │   │   └── test_scanner.rs
│   │   ├── scorer.rs
│   │   └── tests.rs
│   ├── memory
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
│   │   ├── entity_graph
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
│   │   ├── manager
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
│   │   ├── qmd
│   │   │   ├── cache_warming.rs
│   │   │   ├── config.rs
│   │   │   ├── hash.rs
│   │   │   ├── mod.rs
│   │   │   ├── query_builder.rs
│   │   │   ├── reader.rs
│   │   │   ├── search
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
│   │   ├── sqlite_vec_store
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
│   │   ├── sync
│   │   │   ├── diff.rs
│   │   │   ├── manifest.rs
│   │   │   ├── merge.rs
│   │   │   ├── mod.rs
│   │   │   └── push_pull.rs
│   │   ├── telemetry.rs
│   │   ├── tests.rs
│   │   ├── virtual_memory.rs
│   │   └── working.rs
│   ├── mesh
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
│   │   ├── governance
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
│   │   ├── tokenomics
│   │   │   ├── accounting.rs
│   │   │   ├── contracts.rs
│   │   │   ├── economy.rs
│   │   │   ├── mod.rs
│   │   │   ├── rewards.rs
│   │   │   ├── tests.rs
│   │   │   ├── vesting.rs
│   │   │   └── wallet.rs
│   │   └── transport
│   │       └── mod.rs
│   ├── messaging
│   │   ├── discord.rs
│   │   └── mod.rs
│   ├── middleware
│   │   ├── auth.rs
│   │   ├── mod.rs
│   │   └── token_bucket.rs
│   ├── models
│   │   ├── mini_expert.rs
│   │   └── mod.rs
│   ├── node_identity
│   │   ├── bip39_seed.rs
│   │   ├── check_codes.rs
│   │   ├── derive.rs
│   │   ├── hybrid_pack.rs
│   │   ├── mod.rs
│   │   ├── persist.rs
│   │   ├── shamir.rs
│   │   └── vault.rs
│   ├── nodes
│   │   ├── audit.rs
│   │   ├── cert.rs
│   │   ├── mod.rs
│   │   ├── provision.rs
│   │   ├── registry.rs
│   │   └── secrets.rs
│   ├── notifications
│   │   ├── dispatcher.rs
│   │   └── mod.rs
│   ├── observability
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
│   ├── plugins
│   │   ├── mod.rs
│   │   └── runtime.rs
│   ├── polygon_anchor
│   │   ├── abi.rs
│   │   ├── broadcast.rs
│   │   └── mod.rs
│   ├── ports
│   │   ├── inbound
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
│   │   └── outbound
│   │       ├── embedding_port.rs
│   │       ├── health_check_port.rs
│   │       ├── mod.rs
│   │       ├── schema_init.rs
│   │       └── threat_detection_port.rs
│   ├── retrieval
│   │   ├── config.rs
│   │   ├── eval.rs
│   │   ├── gating.rs
│   │   ├── history.rs
│   │   ├── mod.rs
│   │   ├── navigation.rs
│   │   ├── policy.rs
│   │   ├── scoring.rs
│   │   └── tuner.rs
│   ├── scheduler
│   │   ├── daemon.rs
│   │   ├── job.rs
│   │   ├── mod.rs
│   │   ├── retry.rs
│   │   └── retry_tests.rs
│   ├── search
│   │   ├── bm25.rs
│   │   ├── hooks.rs
│   │   ├── hybrid.rs
│   │   ├── mod.rs
│   │   ├── rerank.rs
│   │   └── rrf.rs
│   ├── secrets
│   │   ├── audit.rs
│   │   ├── lending.rs
│   │   ├── local.rs
│   │   ├── local_vault.rs
│   │   ├── mod.rs
│   │   ├── openbao.rs
│   │   ├── store.rs
│   │   ├── tests.rs
│   │   └── vault.rs
│   ├── security
│   │   ├── acl
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
│   │   ├── layers
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
│   │   ├── scanner
│   │   │   ├── entropy.rs
│   │   │   ├── mod.rs
│   │   │   ├── phrase_matcher.rs
│   │   │   └── scanner_impl.rs
│   │   ├── sessions.rs
│   │   ├── threat_store.rs
│   │   ├── tokens.rs
│   │   ├── url_validator.rs
│   │   └── user_store.rs
│   ├── self_manage
│   │   └── mod.rs
│   ├── server
│   │   ├── alerts.rs
│   │   ├── events.rs
│   │   ├── f12_routes.rs
│   │   ├── headless
│   │   │   ├── auth.rs
│   │   │   ├── mod.rs
│   │   │   └── routes.rs
│   │   ├── http
│   │   │   ├── api.rs
│   │   │   ├── context.rs
│   │   │   ├── health.rs
│   │   │   ├── mod.rs
│   │   │   ├── types.rs
│   │   │   ├── v1.rs
│   │   │   └── websocket.rs
│   │   ├── mcp
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
│   │   ├── panel
│   │   │   ├── assets.rs
│   │   │   ├── chat.rs
│   │   │   ├── mod.rs
│   │   │   ├── storage.rs
│   │   │   ├── threads.rs
│   │   │   └── types.rs
│   │   ├── training_routes.rs
│   │   └── v1_api.rs
│   ├── session
│   │   ├── auto_save.rs
│   │   ├── event_mapper.rs
│   │   ├── indexer.rs
│   │   ├── integration_test.rs
│   │   ├── mod.rs
│   │   ├── sharing.rs
│   │   └── types.rs
│   ├── settings
│   │   ├── defaults.rs
│   │   ├── env.rs
│   │   ├── mod.rs
│   │   ├── serialization.rs
│   │   ├── types.rs
│   │   └── validation.rs
│   ├── storage
│   │   ├── migrations.rs
│   │   ├── mod.rs
│   │   └── multi_db.rs
│   ├── sync
│   │   ├── chunks.rs
│   │   ├── manifest.rs
│   │   ├── mod.rs
│   │   └── transport.rs
│   ├── tasks
│   │   ├── mod.rs
│   │   ├── models.rs
│   │   ├── scoring.rs
│   │   ├── session_sync_task.rs
│   │   ├── store.rs
│   │   └── sync.rs
│   ├── telegram
│   │   └── mod.rs
│   ├── tgd
│   │   ├── cache.rs
│   │   ├── consolidation.rs
│   │   └── mod.rs
│   ├── time
│   │   └── mod.rs
│   ├── tools
│   │   ├── gitcore_tools.rs
│   │   ├── kanban.rs
│   │   ├── mod.rs
│   │   ├── search_tools.rs
│   │   └── validation_tools.rs
│   ├── ui
│   │   ├── dashboard.rs
│   │   ├── log_stream.rs
│   │   ├── memory_view.rs
│   │   └── mod.rs
│   ├── utils
│   │   ├── compression.rs
│   │   ├── crypto.rs
│   │   ├── file_traversal.rs
│   │   ├── http.rs
│   │   ├── mod.rs
│   │   └── tauri_utils.rs
│   ├── verification
│   │   ├── auto_verifier.rs
│   │   ├── cycle.rs
│   │   ├── feature_scanner.rs
│   │   └── mod.rs
│   └── workspace
│       ├── config.rs
│       ├── mod.rs
│       ├── ops.rs
│       ├── registry.rs
│       ├── state.rs
│       ├── templates.rs
│       ├── tests.rs
│       └── usage.rs
├── xavier-core/
├── code-graph/
├── panel-ui/
├── parsers/
├── public/
├── scripts/
└── tests/
```

## 3. Core components

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
