---
title: Mesh Network
description: Distributed P2P Memory Synchronization
---

# Xavier Mesh — Distributed P2P Memory Synchronization

The Mesh module implements the foundational layer for connecting Xavier nodes across the internet in a secure, peer-to-peer manner. Each Xavier instance is identified by a unique **NodeID** derived from an Ed25519 public key, enabling cryptographically-authenticated connections without relying on central servers or IP addresses.

## Architecture

```text
┌─────────────────────────────────────────────────────┐
│                  Xavier Mesh Layer                  │
├─────────────────────────────────────────────────────┤
│  Identity   │ NodeID = blake3(ed25519_public_key)   │
│  Protocol   │ XMesh-Sync v1: handshake + manifest   │
│  Transport  │ HTTP REST (Phase 1) → Iroh/QUIC (P2)  │
│  Registry   │ Persistent peer list with metadata    │
└─────────────────────────────────────────────────────┘
```

## Phase 1 Scope

- **NodeIdentity generation and persistence**: Ed25519 keypair used for node identification.
- **Peer registry**: Add, list, and remove trusted peers in a persistent registry.
- **HTTP-based sync transport**: Connect to a remote Xavier node via its HTTP API.
- **XMesh-Sync v1 protocol types**: Standardized types for manifest exchange and chunk requests.
- **CLI commands**:
    - `xavier mesh id`: View local node identity.
    - `xavier mesh add-peer`: Add a trusted peer.
    - `xavier mesh list`: List all known peers.
    - `xavier mesh sync`: Initiate memory synchronization with a peer.

## Future Phases

- **Phase 2**: Iroh QUIC transport with automatic NAT traversal.
- **Phase 3**: Loro CRDT for conflict-free memory merge.
- **Phase 4**: Tor/Yggdrasil transport for anonymous operation.
