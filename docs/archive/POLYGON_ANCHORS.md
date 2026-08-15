# Polygon identity anchors (SWAL Fase 2)

Metadata-only ledger on **Polygon** (Amoy → mainnet). Mesh is never the chain.

## Env vars (never commit values)

| Variable | Required for live | Default / notes |
|----------|-------------------|-----------------|
| `SWAL_POLYGON_RPC_URL` | yes | HTTPS RPC endpoint |
| `SWAL_ANCHOR_KEY` | yes | Hex private key — **never logged**; testnet until audited |
| `SWAL_POLYGON_CHAIN_ID` | no | `80002` (Amoy) |
| `SWAL_ANCHOR_CONTRACT` | yes (live) | Deployed `ISwalIdentityRegistry` address |
| `SWAL_ANCHOR_DRY_RUN` | no | `1` forces mock (also default when RPC/key/contract unset) |
| `SWAL_ANCHOR_BROADCAST` | no | `1` = send tx (requires `--features dao-evm`); else `live-prepared` calldata only |

Without RPC+key+contract (or with dry-run), transport uses mock and writes receipts under `$XAVIER_DATA_DIR/anchors/`.

## Deploy (ops)

```bash
export SWAL_POLYGON_RPC_URL=https://rpc-amoy.polygon.technology
export SWAL_ANCHOR_KEY=0x…   # funded Amoy key — never commit
./docs/SWAL/scripts/deploy-identity-registry-amoy.sh
# → prints SWAL_ANCHOR_CONTRACT=0x…
```

Reference contract: `docs/SWAL/contracts/SwalIdentityRegistry.sol`.

## CLI

```bash
# Identity card hash (public fields only) — dry-run / live-prepared
xavier node anchor [--dry-run] [--data-dir DIR]

# Live broadcast (build with dao-evm)
cargo run -p xavier --features dao-evm -- node anchor
# with: SWAL_ANCHOR_DRY_RUN=0 SWAL_ANCHOR_BROADCAST=1 + RPC/KEY/CONTRACT

# Sealed pack: ciphertext stays off-chain; only content_hash is prepared
xavier node anchor-pack --ciphertext-hex … --meta '{}' [--dry-run]
```

| Mode | Receipt `tx_hash` prefix |
|------|--------------------------|
| dry-run / unset env | `mock:` |
| live env, no broadcast | `live-prepared:` |
| broadcast without `dao-evm` | `live-broadcast-pending:` |
| broadcast + `dao-evm` | `0x…` real tx hash |

## On-chain vs off-chain

- **On-chain:** `content_hash` of identity card / sealed pack (+ submitter address)
- **Off-chain:** seed, vault, ciphertext, business payloads

## ABI (minimal)

```solidity
function anchorIdentity(bytes32 contentHash) external;
function anchorPack(bytes32 contentHash) external;
```

Selectors (verified in `polygon_anchor::abi` tests): `0x4f3066ee`, `0x1581d78e`.
