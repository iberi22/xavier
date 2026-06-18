# Issue: Sovereign Mesh Capability-based Access Control

## Context
As defined in [ADR 006](../docs/ADR/006-sovereign-mesh-boundaries.md), we need to implement a robust capability-based access control system for the internal sovereign network.

## Tasks
1. [ ] Implement `CapabilityToken` generation in `src/mesh/node.rs` (signed with Dilithium-5).
2. [ ] Add `XAVIER_MESH_INTERNAL_TOKEN` environment variable support for auto-pairing nodes.
3. [ ] Integrate `validate_capability` into the `MeshTransport` handshake flow.
4. [ ] Create CLI command `xavier mesh capability grant --grantee <node_id> --scopes <scopes>` to issue tokens.
5. [ ] Add unit tests for expired and malformed tokens.

## References
- `.agents/skills/agentic-memory-ops/SKILL.md` (Governance & Budgets)
- `docs/ADR/006-sovereign-mesh-boundaries.md`
