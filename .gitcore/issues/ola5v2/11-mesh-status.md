# [Ola 5v2 · 11] Mesh: honest component maturity JSON on health/status

> Advances #115 without claiming full mesh completion. Gold-standard Jules issue.

## Web Research Required (Jules must search the web)

1. **Detailed health/ready endpoints** — search: `kubernetes style health components status endpoint design 2024`, distinguish liveness vs detailed capability maps.
2. **libp2p-rust peer count APIs** — search: `libp2p rust swarm connected peers 2024` (only if reading live state is cheap).
3. **Feature maturity matrices** — search: `software capability maturity flags API JSON 2024`.

## Exact Technical Context

- Mesh code: `src/mesh/` (`libp2p_transport.rs`, `acl.rs`, `governance.rs`, `tokenomics/`, `iroh_transport.rs`, …)
- Health system: `src/observability/health.rs` + HTTP `/health` wiring in `src/cli/server.rs`
- Prefer enriching mesh section of `/health` OR adding `GET /v1/mesh/status`

Example payload (booleans must be **honest**):

```json
{
  "http_transport": true,
  "libp2p": true,
  "acl": true,
  "tokenomics": true,
  "onchain_governance": false,
  "data_commons": true,
  "iroh_quic": false,
  "tor": false
}
```

Use compile-time/feature flags or cheap runtime probes; do not fake on-chain.

> CRITICAL: DO NOT implement on-chain DAO, Tor, or full Iroh rewrite. Minimal status surface only. NEVER `.patch` files. No xavier-core/.

## Problem

EPIC #115 progress is opaque; operators lack machine-readable maturity flags → overconfidence.

## Acceptance Criteria

- [ ] HTTP JSON exposes mesh component map
- [ ] Unit test asserts required keys exist
- [ ] `onchain_governance` is false unless real code path exists
- [ ] `cargo check --workspace` 0 errors

## Files to Modify

| File | Change |
|---|---|
| `src/observability/health.rs` and/or handlers + `server.rs` | status field/route |
| tests | shape |

## Verification

```bash
cargo check --workspace
```

## Dependencies and Merge Order

- **Depends on:** nothing
- **Can run in parallel with:** most issues
