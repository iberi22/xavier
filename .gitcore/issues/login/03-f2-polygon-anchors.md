# DL-03 — F2 Polygon identity / pack anchors

| Campo | Valor |
|-------|--------|
| **% validado** | **90%** |
| **Estado** | done shippable · ops Amoy residual |

## Scope

Hash canónico identidad, sealed-pack content_hash, dry-run default, live-prepared calldata, broadcast `dao-evm`, receipts 0600, CLI `anchor` / `anchor-pack`.

## Aceptación validada

- [x] Solo metadata hash on-chain (mock/dry-run)
- [x] Ciphertext ausente de receipts
- [x] ABI selectors + prepare calldata
- [x] Broadcast module behind `dao-evm`
- [ ] Deploy Amoy + tx real (ops / key funded)

## Tests

| Suite | Resultado |
|-------|-----------|
| `polygon_anchor::*` | PASS (8) |
| E2E `e2e_f2_polygon_anchor_dry_run_receipts` | PASS |
| `cargo check -p xavier --features dao-evm` | PASS (sesión previa) |

## Paths

`src/polygon_anchor/`, `docs/POLYGON_ANCHORS.md`, monorepo `docs/SWAL/scripts/deploy-identity-registry-amoy.sh`
