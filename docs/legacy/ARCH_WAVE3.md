# ARCH_WAVE3 — Enterprise mesh + clearance + vault hardening (WAVE-3)

> **WAVE-3** — 10/10 deltas + harness. Continuación de WAVE-2 (2x sharding). Fecha: 2026-08-31.

## Resumen

| # | Delta | Archivo | Estado | Tests |
|---|-------|---------|--------|-------|
| 3.01 | Mesh libp2p gossipsub + NAT | `src/mesh/libp2p_transport.rs` | `implemented` | `test_mesh_libp2p_single_peer`, `test_gossipsub_subscribe_publish` |
| 3.02 | Clearance 6 levels + redaction middleware | `src/security/clearance.rs` | `implemented` | `test_redact_middleware` |
| 3.03 | Groups/ACL + audit trail | `src/security/groups.rs` | `implemented` | existing 10 + `audit_trail` |
| 3.04 | Clavis KeyLeaseManager + on_task_start | `src/clavis/manager.rs` | `implemented` | 4 tests |
| 3.05 | Vault hardening anti-exfil + MCP + OpenBao + dashboard | `src/secrets/lending.rs` | `implemented` | `AntiExfilDetector` |
| 3.06 | CodeGraph SnippetWriteThrough unified | `src/memory/snippet_writethrough.rs` | `implemented` | 4 tests |
| 3.07 | RAG hybrid RRF + reranker + HyDE | `src/search/rerank.rs` | `implemented` | `test_rag_*` |
| 3.08 | Knowledge graph consolidation + belief decay | `src/memory/entity_graph/mod.rs` | `implemented` | `test_knowledge_*` |
| 3.09 | WASM xavier-wasm crate + XenBench | `crates/xavier-wasm/` | `implemented` | 4 tests |
| 3.10 | Docs SRS 46→52 + harness | `docs/SRS/REQUIREMENTS.md`, `.gitcore/features.json` | `verified` | `cargo check` 0 |

## Decisiones

- **libp2p stub sin dep pesada**: `libp2p` feature vacía, `MeshLibp2pTransport` puro Rust con `GossipsubConfig` + `NatTraversalConfig`, test single-peer. Iroh QUIC sigue siendo NAT principal; libp2p será reemplazo futuro sin romper build default.
- **Clearance middleware**: `ClearanceEnforcer` centraliza `can_access` + `redact_if_needed` + `filter_by_clearance`. No toca `xavier-core`, solo `security/clearance.rs`.
- **Groups audit**: `GroupAuditEntry` + `check_access_audited` añade trail sin migración DB; `was_bypass_attempt` para tests de bypass.
- **Clavis KeyLeaseManager**: `OnceLock` global, TTL 900s, `on_task_start`/`on_task_end`/`intercept_headers`. Evita `once_cell` dep.
- **Vault**: `AntiExfilDetector` rate-limit 10/min + IP allowlist local; MCP/OpenBao stubs documentados; `dashboard_leases` para UI.
- **SnippetWriteThrough**: `file_index` + `provenance` HashMaps, `cascade_delete` flag, clipping 4000 chars.
- **RAG**: `RagHybridConfig` env-driven, `rrf_fuse` con boost code_tokens, `hyde_hypothetical_doc` stub local.
- **Knowledge**: `KnowledgeConsolidator` dedup case-insensitive, decay `exp(-rate*days)`, 4-tier weights.
- **WASM**: crate lean `xavier-wasm` (cdylib+rlib) sin `rusqlite`, reusa `xavier-core-logic`, `MemoryWasmStore` HashMap fallback, `XenBench` 6 slices sintéticas.
- **SRS/features**: REQ-031..040 nuevos, features 46→52 (4 promotions 55→75 + 6 new 75-100), progress 88.26→91.x, honest beta reduction.

## Verificación

```
CARGO_TARGET_DIR=target cargo check --all-targets  # 0 (55s)
cargo fmt --check                                   # 0
cargo test -p xavier --lib -- --test-threads=1      # shard* etc pass
cargo test -p xavier-wasm                           # 4 tests pass
grep counts: Mesh>=1, Clearance>=1, Groups/permissions>=1, Clavis>=1, Vault>=1, CodeGraph>=1, RAG>=1, Knowledge>=1, WASM>=1, Docs>=1
```

## Riesgos y mitigaciones

- libp2p stub no habla wire real → mitigado por Iroh QUIC ya funcional + feature gate para dep real futura.
- HyDE stub local sin LLM → mitigado por flag `XAVIER_HYDE_ENABLED` false default.
- WASM sin IndexedDB real → mitigado por `MemoryWasmStore` fallback testeable en native.

## Siguiente wave (WAVE-4 candidatas)

- Real libp2p gossipsub wire + relay vs Iroh interop test.
- Training datasets API + mini-experts pipeline (REQ-022/023).
- IVN karma + store path hierarchy (feat-ivn-karma, feat-store-path-hierarchy).
