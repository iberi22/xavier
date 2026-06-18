# Issue: Sovereign Mesh Maintainer Bounties & Reward Ledger

## Context
Active maintainers and contributors should be rewarded for their work. Rewards must be auditable via the Reward Ledger.

## Tasks
1. [ ] Finalize `RewardLedger` implementation in `src/mesh/tokenomics/ledger.rs` with SQLite persistence.
2. [ ] Implement a `BountyRegistry` for tracking open tasks and claimed rewards.
3. [ ] Add Dilithium-5 signing to all `LedgerEntry` records.
4. [ ] Create a `xavier mesh rewards audit` command to verify the local ledger against the network.
5. [ ] Integrate `RewardEngine` with the telemetry and sync services.

## References
- `src/mesh/tokenomics/rewards.rs`
- `src/tasks/scoring.rs`
