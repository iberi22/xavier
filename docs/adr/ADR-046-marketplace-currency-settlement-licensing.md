# ADR-046 — Marketplace currency, settlement and licensing

| Campo | Valor |
|-------|--------|
| **ID** | ADR-046 |
| **Estado** | Propuesto |
| **Fecha** | 2026-10-16 |
| **Autores** | Xavier architecture contributors |
| **Relacionados** | #3206 (MKT-01); MKT-02 #3207 onward; ADR-013 (compute market, rewritten); `docs/site/src/content/docs/modules/data_commons.md` |

## Context

The data marketplace (`src/data_commons/*`, `/v1/marketplace/*`) has no decided money model. Three owner questions block implementation: which currency and payment rail, whether settlement is on-chain or off-chain, and which licenses a seller may attach to a listing. The downstream marketplace issues (MKT-02 onward) need these answers as configuration so they can be built and switched on without another design round.

Constraints:

- Real money requires terms of service that do not exist yet, and Xavier Cloud already has a merchant of record (Polar) for its own billing.
- `$SWAL` settlement depends on the canonical-token decision and a legal opinion. Neither exists yet.
- The v2 marketplace must not change behaviour for existing users. `src/data_commons/*` and `/v1/marketplace/*` keep working until the switch-over issue lands.

## Options considered

### Currency and payment rail

| Option | Description | Cost / complexity |
|--------|-------------|-------------------|
| A. Internal credits (non-transferable) | Credits earned by contributing data, spent on data. No cash out. | Low: a ledger row per credit movement. No payment provider. |
| B. USD via Polar now | Card payments through the existing merchant of record. | Medium: checkout, webhooks, refunds, tax handling, and terms before launch. |
| C. `$SWAL` token now | Settle listings in the SWAL token. | High: canonical-token ADR and legal opinion are prerequisites. |

### Settlement

| Option | Description | Cost / complexity |
|--------|-------------|-------------------|
| A. Off-chain ledger | Balances and settlements in the Xavier SQLite store. | Low. Auditable through the store, no chain dependency. |
| B. On-chain escrow / settlement | Settlement on a chain, with escrow contracts. | High. Needs the canonical-token decision, contract audit, and legal review. |

### License menu

| Option | Description | Cost / complexity |
|--------|-------------|-------------------|
| A. Fixed menu | `CC-BY-4.0`, `CC-BY-NC-4.0`, `SWAL Data License v0`. | Low. Enumerated values, validated on write. |
| B. Free-text license | Seller types any license string. | Low to build, high legal risk. Rejected. |
| C. Per-listing custom terms | Each listing carries its own contract text. | High review and support cost. Deferred. |

## Decision

1. **Currency (default):** v1 uses non-transferable internal credits. Credits are earned by contributing data and spent on data. They cannot be withdrawn or transferred between accounts. The value is set by `market.currency`, default `internal_credits`.
2. **Real money (later):** USD through Polar, the merchant of record already chosen for Xavier Cloud. This is enabled only after marketplace terms are published. Config value `usd_polar`, default disabled.
3. **$SWAL settlement:** not available until the canonical-token ADR is accepted and a legal opinion is on file. Config value `swal_token`, default disabled.
4. **Settlement (default):** off-chain. Balances and settlements live in the `xavier-market` SQLite store. On-chain settlement is not part of v1. Config key `market.settlement`, default `off_chain`.
5. **License menu (default):** a fixed list of three licenses:
   - `CC-BY-4.0`
   - `CC-BY-NC-4.0`
   - `SWAL Data License v0`: purpose-limited, time-limited, no re-identification.

   Free-text and per-listing custom licenses are rejected for v1. The seller must warrant that they hold the rights to the data and have consent from the data subjects where needed. Config key `market.licenses`, default the three values above.
6. **Legal text:** the text of `SWAL Data License v0` and the seller warranty is a placeholder until a lawyer reviews it. No listing may be published under real terms until that review is recorded.
7. **Feature gate:** the v2 marketplace sits behind the `market-v2` cargo feature and the `XAVIER_MARKET_V2=beta` runtime flag. Both are off by default. `src/data_commons/*` and `/v1/marketplace/*` are unchanged by this decision.

### Config keys

These key names are fixed by this ADR. MKT-02 and later issues implement them. No key exists in code yet.

| Key | Default | Allowed values | Notes |
|-----|---------|----------------|-------|
| `market.currency` | `internal_credits` | `internal_credits`, `usd_polar`, `swal_token` | `usd_polar` requires published terms. `swal_token` requires the canonical-token ADR and a legal opinion. |
| `market.settlement` | `off_chain` | `off_chain`, `on_chain` | `on_chain` is not implemented in v1 and is rejected at startup. |
| `market.licenses` | `["CC-BY-4.0", "CC-BY-NC-4.0", "SWAL Data License v0"]` | subset of the fixed menu | Any other value is rejected on write. |
| `market.legal_text_status` | `placeholder` | `placeholder`, `reviewed` | Must be `reviewed` before real-money or public listings. |
| `XAVIER_MARKET_V2` | `off` | `off`, `beta` | Runtime flag. Also requires the `market-v2` cargo feature to be compiled in. |

## Simulation

Not run in this PR. The three choices above are owner decisions recorded in #3206 with the defaults applied. This ADR is therefore `Propuesto`. Its status moves to `Aceptado` only after the required review. The status must not move to `Aceptado` before the legal opinion for `$SWAL` settlement exists.

## Assumptions

| Parameter | Value used | Source |
|-----------|------------|--------|
| Merchant of record for USD | Polar | Issue #3206, owner decision |
| `$SWAL` gating | Canonical-token ADR and legal opinion required | Issue #3206, owner decision |
| Existing marketplace routes stay live | `src/data_commons/*`, `/v1/marketplace/*` | Issue #3206, Decoupling rule |
| Legal text | Placeholder until lawyer review | Issue #3206, owner decision |

## Consequences

- **Positive:** v1 avoids payment-provider, tax, and token-legal work. The config keys let later issues implement against fixed names. The license menu is closed, so the validator is simple and auditable.
- **Negative / cost:** credits cannot be cashed out, so sellers have no monetary return in v1. The license menu cannot express custom terms. Real-money launch waits for published terms.
- **What would invalidate this decision:** a legal opinion that internal credits are a regulated instrument; an owner decision to launch USD payments before the terms are published; a canonical-token decision that makes on-chain settlement mandatory; or a seller request that the fixed menu cannot satisfy.

## Verification

- Check that every default in this ADR is a config key in code once MKT-02 lands, with the same name and default.
- Check that `XAVIER_MARKET_V2` is off in a fresh build and in CI, and that existing `/v1/marketplace/*` tests pass unchanged.
- Review date: 2026-11-16, or when the legal review or canonical-token ADR lands, whichever comes first.
