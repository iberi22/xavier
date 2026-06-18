# Issue: Sovereign Mesh Deterministic Scoring Enhancements

## Context
Task and data scoring must be deterministic to ensure fair rewards across the network.

## Tasks
1. [ ] Expand `src/tasks/scoring.rs` with versioned `ScoringParams` that can be updated via Governance.
2. [ ] Implement `DataQualityScorer` for memory chunks (shannon entropy, token density).
3. [ ] Add `ConsensusScorer` for validating peer-submitted telemetry.
4. [ ] Create a regression test suite for all scoring functions using static fixtures.
5. [ ] Integrate scoring with the `RewardLedger`.

## References
- `src/tasks/scoring.rs`
- `docs/XAVIER_DATA_COMMONS_ARCHITECTURE.md` (Pricing dinámico)
