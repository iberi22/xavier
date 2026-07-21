---
title: "Xavier Mesh P2P Network & Data Commons"
description: "Distributed P2P memory synchronization with encrypted telemetry, Ed25519 identity, Deep Permissions (ACL), and Data Commons reward funnel"
---

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
Xavier Mesh represents a secure, distributed peer-to-peer (P2P) network and shared Data Commons for decentralized context sharing, memory synchronization, and collaborative AI reasoning. It includes robust node-level authentication, cryptographic pairing, deep access control lists (ACLs), and automated feature maturity tracking.

## Architecture & Design
Nodes utilize Ed25519 keys for identity and establish secure sessions via base64 encoded SWAL (Secure Workspace Access License) tokens. Real-time connections authenticate and validate signatures using `NodeIdentity::verify`. Active sharing consents are logged in `mesh_active_consents.json` with token revocations monitored dynamically via `mesh_token_revocations.json`. Deep federated queries propagate cleanly with hop controls and cycle exclusions to prevent routing loops.

## Implementation Paths
- `src/mesh/` (P2P networking, SWAL authentication, data consent management, and context bridge registry)
- `src/data_commons/` (tokenomics, reputation engines, and governance wrappers)
- `tests/mesh_integration.rs` (end-to-end multi-node query verification)

## Sub-features
- **mesh-mvp-http-libp2p-acl:** Standard HTTP routing overlay + libp2p network backbone + namespace and segment-wise folder path ACL matching.
- **mesh-maturity-status:** Integrated `MeshMaturityReport` tracing operational percentages across http_transport (100%), libp2p (10%), acl (90%), tokenomics (40%), and onchain_gov (0%).
- **mesh-phase2-iroh-tor-onchain:** Research track for advanced NAT traversal via Iroh/QUIC, Tor onion routing, and blockchain-based token rewards (EPIC #115).

## Test References
- `tests/mesh_integration.rs` (joins, query fan-outs, and parallel queries).
- Node identity signature verification and token validation unit tests in `src/mesh/auth.rs`.

## Known Issues & Notes
- On-chain features and libp2p NAT traversals are flagged as low-maturity/planned research items, and do not block 1.0 MVP core services.
- Data Consent Protocol handles historical base64-encoded tokens gracefully by hashing payloads into unique SHA-256 fallback IDs.

### Functional Mesh P2P Example
Share a local workspace database over the P2P Mesh Network:

```bash
# Share workspace via HTTP endpoint
curl -X POST "http://localhost:8006/v1/mesh/workspaces/share" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "workspace_id": "workspace-123",
    "namespaces": ["core::auth", "cognitive::beliefs"]
  }'
```

Verify P2P Node Identity:
```bash
xavier mesh status
```
