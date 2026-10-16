# Marketplace (v2) — overview

Status: **beta, off by default.** The v2 marketplace is behind the `market-v2` cargo feature and the `XAVIER_MARKET_V2=beta` runtime flag. The existing `src/data_commons/*` and `/v1/marketplace/*` routes keep working unchanged.

## Decisions

The money model, settlement and license menu are fixed in [ADR-046 — Marketplace currency, settlement and licensing](../adr/ADR-046-marketplace-currency-settlement-licensing.md).

Defaults for v1:

- Currency: non-transferable internal credits (`market.currency = internal_credits`).
- Real money: USD through Polar, only after marketplace terms are published (`usd_polar`, disabled).
- `$SWAL` settlement: waits for the canonical-token ADR and a legal opinion (`swal_token`, disabled).
- Settlement: off-chain (`market.settlement = off_chain`).
- Licenses: `CC-BY-4.0`, `CC-BY-NC-4.0`, `SWAL Data License v0`. Seller warrants rights and consent. Legal text is a placeholder until a lawyer reviews it (`market.legal_text_status = placeholder`).

## Configuration keys

| Key | Default |
|-----|---------|
| `market.currency` | `internal_credits` |
| `market.settlement` | `off_chain` |
| `market.licenses` | `["CC-BY-4.0", "CC-BY-NC-4.0", "SWAL Data License v0"]` |
| `market.legal_text_status` | `placeholder` |
| `XAVIER_MARKET_V2` | `off` |

Keys are defined by ADR-046. Implementation lands in the `xavier-market` crate (MKT-02 onward).
