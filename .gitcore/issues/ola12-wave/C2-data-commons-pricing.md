# C2: Data Commons pricing model (15% → 35%)

## Problem

Data Commons economy is at 15% (PR #217 merged). No pricing model exists.
`DATA-MARKETPLACE.md` has the vision but no implementable economic parameters.

## Solution

Design and implement a pricing model for data packages.

### Pricing tiers

| Tier | Storage | Price | Revenue Share |
|------|---------|-------|---------------|
| Free | 100MB | $0 | — |
| Colaborador | 10GB | $5-10/mo | 25% to provider |
| Colaborador+ | 100GB | $20-30/mo | 25% to provider |

### Data package pricing

- Per-package fixed price (set by provider)
- Platform fee: 10% of sale price
- Provider keeps 90%
- $SWAL staking for provider reputation boost

### Steps

1. Create `src/data_commons/pricing.rs` with pricing structs
2. Implement `calculate_price(package_size, tier, reputation)` function
3. Add `revenue_share()` calculation
4. Wire to existing marketplace endpoints
5. Add unit tests for pricing logic
6. Update DATA-MARKETPLACE.md with concrete numbers

## Acceptance

- [ ] Pricing structs defined with all tiers
- [ ] calculate_price returns correct values for each tier
- [ ] Revenue share calculation verified
- [ ] Unit tests pass
- [ ] DATA-MARKETPLACE.md updated with real numbers

## Files

- `src/data_commons/pricing.rs` (new)
- `src/data_commons/marketplace.rs` (modify)
- `docs/benchmark/DATA-MARKETPLACE.md` (modify)

## Dependencies

None (standalone island)
