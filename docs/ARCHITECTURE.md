# Xavier Architecture

> **GitCore Protocol v3.8** | Feature tracking: `.gitcore/features.json` (52/52 stable 100% 2026-09-01)
> **Status:** Canonical v2.0 — 2026-09-02 (New Web Spaces aligned)
> **Stack:** Rust + SQLite_vec + BM25 + Iroh QUIC + Loro CRDT + ML-DSA-65

## Core Modules (canonical, English)

```
src/
├── memory/        — RAG engine (SQLite_vec + BM25 + semantic + belief graph + dedup 0.92)
├── retrieval/     — Search, scoring, gating, navigation policies (RRF, HyDE, rerank)
├── embedding/     — GLLM / OpenAI pipeline (1536d) + local nomic 768d fallback
├── codebase/      — Code graph (Tree-sitter AST, call chains, blast radius)
├── mesh/          — P2P mesh (Iroh QUIC primary -> WebRTC -> Tor -> BYO CF Relay, PrivateMeshRegistry, NetworkAcl)
├── espacio/       — Spaces (NEW 2026-09-02): isolated WorkspaceState per Space, PrivateMesh + channel Loro CRDT
├── pack/          — .swalpack export/import (CBOR zstd: memories.jsonl + vectors.sqlite + code_graph.jsonl) (NEW)
├── data_commons/  — Marketplace DataMarketplace {list/query/revoke} + pricing + reputation (shared with Spaces)
├── security/      — Clearance 6 levels + RedactionEngine + groups ACL + audit trail
├── clavis/        — KeyLeaseManager + vault hardening anti-exfil + Shamir DEK
├── server/        — Axum REST :8006 + MCP :8100 + Maloca bridge
├── storage/       — SQLite WAL, multi_db per Space, migrations v1-v5
└── health/        — Heartbeat, mesh telemetry, readiness
```

- **memory + retrieval**: hybrid BM25+vector RRF, zone booster, belief decay, entity graph, snippet 100 chars + page-in.
- **mesh + espacio**: PrivateMeshRegistry wallet_id=SHA256(pubkey), MeshNetwork N members, CrossGrant resource->node expiry, Loro CRDT channel per Space, Iroh QUIC fan-out 3, OfflineQueue.
- **pack + data_commons**: .swalpack CBOR, DataMarketplace 500 rows test, pricing tier, reputation, OfferBlock gossip, SDC hash anchor.
- **security + clavis**: clearance, redaction, Shamir 2-of-3, HardwareVault.

## Feature Maturity (2026-09-02 aligned)

| Feature | Status | Notes |
|---------|--------|-------|
| Hybrid Search + RRF + HyDE | Stable 100% | automated |
| Spaces core (T-01) | Planned P0 | WorkspaceRegistry per Space, isolation test |
| Pack RAG .swalpack (T-04) | Planned P1 | CBOR + dedup + ML-DSA verify |
| Marketplace folders (T-08) | Planned P1 | extend existing DataMarketplace |
| Graph navigable (T-06) | Planned P1 | ACL-checked |
| Closed P2P E2E (T-07) | Planned P1 | Iroh->Tor->CF Relay |
| Wraps virtual + vault (T-11/12) | Planned P2 | council 50%+1 |
| Karma hybrid + oracle 2/3 (T-13) | Planned P2 | tier_lock OK |
| SDC Directory Chain (T-14) | Planned P2 | gara-chain skeleton + PoUW-E |
| Existing 52/52 | Stable 100% | keep |

Legacy docs moved to `docs/legacy/` (ARCH_DISTRIBUTED_2X, ARCH_HEXAGONAL_FALLBACK, ARCH_WAVE3/4, advanced-settings, HEARTBEAT, MEMORY, TOOLS). Single source of truth is this file + SRC.md + FEATURE_STATUS.md.

## New Web Spaces — Integration

Spaces are isolated WorkspaceState per `xavier://{space_id}/{appId}/{instanceId}` (namespace.rs). Share creates PrivateMesh + Loro channel + marketplace OfferBlock. Search is hybrid BM25+vector with karma ranking. Sibling nodes per wallet (no parent, GOAL #11) keep mesh always online.

See `docs/SWAL/NUEVA_WEB_SPACES_XAVIER_2026-09-02_EN.md` for HU-01..08 and `TASKS_NEW_WEB_2026-09-02_EN.md` for 14 tasks.
