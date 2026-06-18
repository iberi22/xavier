# Issue: Sovereign Mesh Governance Integration

## Context
Xavier Mesh requires a decentralized governance system to manage protocol parameters and community proposals.

## Tasks
1. [ ] Port `GovernanceEngine` from `src/data_commons/governance.rs` to a mesh-wide service.
2. [ ] Implement `XIP` (Xavier Improvement Proposal) message types in `src/mesh/protocol.rs`.
3. [ ] Add voting hooks to the Panel UI (`panel-ui/src/api/client.ts`).
4. [ ] Implement bicameral tallying logic (Users + Council).
5. [ ] Integrate `EigenTrust` reputation scores into voting weight calculation.

## References
- `docs/GOVERNANCE_VISION.md`
- `src/data_commons/governance.rs`
