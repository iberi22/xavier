# Login descentralizado — Issues GitCore (sesión 2026-07-28)

Epic local (no GH requerido para cierre documental).  
Feature: `feat-decentralized-login` · **95%** overall (validado con unit + E2E).

| ID | Título | Fase | % validado | Tests | Estado |
|----|--------|------|------------|-------|--------|
| [DL-01](./01-f0-vault-bip39-shamir.md) | Vault BIP39 + Shamir + CLI | F0 | **95%** | unit `node_identity::*` + persist + E2E F0 | done |
| [DL-02](./02-f1-mesh-challenge-pro.md) | Mesh challenge + Pro gate | F1 | **95%** | unit challenge/namespace/pro_gate + E2E F1 | done |
| [DL-03](./03-f2-polygon-anchors.md) | Polygon hash anchors | F2 | **90%** | unit `polygon_anchor::*` + E2E F2 dry-run | done (ops residual) |
| [DL-04](./04-f3-hybrid-pq-packs.md) | Hybrid Ed25519+ML-DSA packs | F3 | **100%** | unit hybrid_pack + E2E F3 | done |
| [DL-05](./05-f4-bio-zkp-research.md) | Bio/ZKP research track | F4 | **5%** | ADR only | research |
| [DL-06](./06-apps-heartbeat-device-key.md) | Apps heartbeat + device_key API | F0/F1 | **90%** | `@swal/node` 12 tests (maloca) | done (UI residual) |

## Evidencia de pruebas (Xavier)

Ver [TEST_EVIDENCE.md](./TEST_EVIDENCE.md) · suite E2E: `cargo test -p xavier --test decentralized_login_e2e`

## Criterio de %

- **done / X%**: código + tests verdes; residual documentado (ops/UI/research).
- No se cuenta F4 en el 95% shippable del feature.
