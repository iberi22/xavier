# [Ola 5 · 11] Mesh: status endpoint maturity report (honest %)

> Advances #115 without claiming full mesh done

## Exact Technical Context
- src/mesh/* has many modules
- Expose or enrich GET health/mesh status with component flags: http_transport, libp2p, acl, tokenomics, onchain_gov (false)
- Update feat notes only if code path exists — prefer small JSON under /health or /v1/mesh/status

## Acceptance Criteria
- [ ] Structured mesh maturity fields available via HTTP
- [ ] Tests for JSON shape
- [ ] cargo check --workspace
- [ ] DO NOT implement full on-chain DAO here

## Merge order
Independent.
