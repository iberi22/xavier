# Mesh Capability Status

This page describes what the Xavier mesh does today, and what it does not. It is a public summary of the capabilities of the P2P mesh network implementation.

Last reviewed: `a44fd8a0a` (2026-10-15). Statuses are from reading the code. No test was run for this page.

| Capability | Status | Evidence |
|---|---|---|
| Peer discovery | PARTIAL: static peer registry and add-peer | (`src/cli/commands/mesh.rs`) |
| NAT traversal | UNVERIFIED: a module exists, behaviour not measured | (`src/mesh/p2p/nat_traversal.rs`) |
| libp2p transport | STUB: compiled, reports 0 in health | (`src/mesh/maturity.rs`, `Cargo.toml` `libp2p = []`) |
| iroh/QUIC transport | PARTIAL, opt-in: behind feature `mesh`, off by default | (`Cargo.toml` `mesh = ["dep:iroh"]`) |
| Node identity (Ed25519) | BUILT | (`src/mesh/node.rs`) |
| ML-DSA signing | NOT SHIPPED: a 32-byte commitment field only | (`src/mesh/node.rs`) |
| Pairing | PARTIAL: pairing code and join in the CLI | (`src/cli/commands/mesh.rs`) |
| Memory sync over HTTP | PARTIAL: HTTP delta sync | (`src/mesh/`) |
| CRDT merge (Loro) | MISSING: planned, not started | none |
| Memory duplication on sync round trip | OPEN BUG: memories can duplicate after a round trip; no regression test yet | none |
| Data consent and sanitizer | PARTIAL: used by the sync filter; unit tests only | (`src/mesh/data_consent.rs`, `src/mesh/data_sanitizer.rs`) |
| HTTP API | BUILT: `/v1/mesh/*` routes mounted | (`src/cli/server.rs`) |
| CLI | BUILT: id, add-peer, list, remove-peer, ping, sync, status, pairing-code, join | (`src/cli/commands/mesh.rs`) |
| Health and maturity | PARTIAL: real inputs; libp2p fixed at 0 | (`src/mesh/maturity.rs`) |
| Public directory | PARTIAL: `GET /v1/mesh/public/nodes` | (`src/cli/server.rs`) |
| Mesh integration tests in CI | PARTIAL: they run with feature `mesh`, in a non-blocking wave only | (`tests/mesh_iroh_test.rs`, `tests/mesh_node_lifecycle_test.rs`, `tests/mesh_peer_registry_test.rs`) |

**Not on this page:** rows about the security posture of storage and transport are not listed here. The owner decides how those are published.

**How to verify a row:** open the evidence path, and check the cited symbol.
