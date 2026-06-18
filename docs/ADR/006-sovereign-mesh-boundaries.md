# ADR 006: Xavier Sovereign Mesh Boundaries

## Status
Proposed

## Context
Xavier currently uses a unified mesh sync protocol for sharing memory chunks between nodes. As we move towards a "Sovereign Mesh," we need to distinguish between internal replication (high-trust, sensitive data) and external participation (low-trust, governance, and data commons).

## Decision
We will partition the Xavier Mesh into two distinct functional boundaries:

### 1. Internal Sovereign Network (Replication)
- **Purpose**: High-fidelity replication of private memory, settings, and agent states.
- **Access Control**: Strictly enforced via **Capability Tokens** derived from the Node's Wallet. Raw shared secrets are deprecated in favor of signed capability grants.
- **Topology**: Main Xavier node (Authority) -> Mirror Nodes (Replicas).
- **Security**: Data is encrypted using the recipient node's public key (Kyber-1024) and signed by the sender (Dilithium-5).

### 2. External Governance & Data Commons Network
- **Purpose**: Sanitized telemetry, task validation, consensus, and reward distribution.
- **Access Control**: Open participation gated by Node Reputation and activity.
- **Data Policy**: Only anonymized and sanitized telemetry/context offers are shared. Private memory NEVER leaves the node to the external network.
- **Incentives**: Participation earns XP tokens, auditable through an append-only ledger.

## Consequences
- Nodes must maintain separate ACLs for internal vs. external peers.
- All mesh protocol messages must now include a `capability_token` for internal operations.
- The `DataSanitizer` must be strictly enforced at the external network boundary.
- Improved auditability of data movement.
- Clearer path for Enterprise vs. Community deployments.

## Implementation Details
- **Wallet**: Use existing `src/data_commons/wallet.rs` and `src/mesh/node.rs`.
- **Capabilities**: New `CapabilityToken` struct in `src/mesh/protocol.rs`.
- **Validation**: Logic in `src/mesh/acl.rs`.
