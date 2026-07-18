# [Ola 5 · 08] Dependabot: cargo high severity bumps

> Part of #478 — known high: jsonwebtoken, protobuf, undici is npm, opentelemetry_sdk, serde_with

## Exact Technical Context
- Cargo.toml / Cargo.lock at workspace root
- Prefer minimal version bumps; run cargo check --workspace

## Acceptance Criteria
- [ ] Address as many **high** cargo alerts as possible without breaking build
- [ ] cargo check --workspace 0 errors
- [ ] List remaining unfixed highs with reason
- [ ] DO NOT touch panel-ui in this PR

## Merge order
After 07 preferred (avoid dual lock churn) or sequential.
