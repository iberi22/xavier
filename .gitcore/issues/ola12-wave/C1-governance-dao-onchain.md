# C1: Governance DAO on-chain stub (5% → 25%)

## Problem

Mesh EPIC #115 has Governance DAO at 5%. No on-chain governance exists.
The mock DAO (30%) provides the interface but no real blockchain integration.

## Solution

Implement a minimal on-chain governance stub for Polygon Amoy testnet.

### Contract scope (MVP)

- `propose(string description, bytes calldata action)` → proposal ID
- `vote(uint256 proposalId, bool support)` → vote recorded
- `execute(uint256 proposalId)` → execute if quorum met
- Quorum: 51% of staked $SWAL nodes
- Voting period: 48 hours

### Steps

1. Design contract interface in `docs/adr/ADR-014-governance-dao-onchain.md`
2. Implement Solidity contract in `mesh/governance/contracts/XavierDAO.sol`
3. Add Rust binding via `alloy` or `ethers-rs` in `src/mesh/governance/onchain.rs`
4. Wire to existing mock DAO interface in `src/mesh/governance/mod.rs`
5. Add integration test against local Anvil node
6. Update `feature-maturity.json`: governance DAO on-chain 5% → 25%

## Acceptance

- [ ] Solidity contract compiles and passes basic tests
- [ ] Rust binding can propose + vote against Anvil
- [ ] Existing mock DAO tests still pass
- [ ] ADR-014 documents the decision
- [ ] feature-maturity.json updated

## Files

- `mesh/governance/contracts/XavierDAO.sol` (new)
- `src/mesh/governance/onchain.rs` (new)
- `src/mesh/governance/mod.rs` (modify)
- `docs/adr/ADR-014-governance-dao-onchain.md` (new)
- `.xavier/feature-maturity.json`

## Dependencies

None (standalone island)
