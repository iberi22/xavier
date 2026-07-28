# TEST EVIDENCE — feat-decentralized-login (2026-07-28)

Host: local · Branch: `main` · Feature overall: **95%**

## Summary

| Suite | Command | Passed | Failed |
|-------|---------|--------|--------|
| E2E login pipeline | `cargo test -p xavier --test decentralized_login_e2e` | **5** | 0 |
| Persist F0 | `cargo test -p xavier --test node_fase0_persist` | **2** | 0 |
| Unit `node_identity` | `cargo test -p xavier --lib node_identity` | **16** | 0 |
| Unit `polygon_anchor` | `cargo test -p xavier --lib polygon_anchor` | **8** | 0 |
| Unit `mesh::challenge` | `cargo test -p xavier --lib challenge::` | **2** | 0 |
| Unit `mesh::namespace` | `cargo test -p xavier --lib namespace::` | **3** | 0 |
| Unit `mesh::pro_gate` | `cargo test -p xavier --lib pro_gate::` | **5** | 0 |
| `@swal/node` (maloca) | `node --test src/*.test.ts` | **12** | 0 |
| **Total Xavier login** | | **41** | **0** |
| **+ maloca** | | **53** | **0** |

## E2E cases (`tests/e2e/decentralized_login_e2e.rs`)

| Test | Cubre |
|------|--------|
| `e2e_f0_create_persist_recover_identity` | BIP39/Shamir/vault/recover/check-codes |
| `e2e_f1_mesh_challenge_namespace_pro_gate` | challenge one-shot, namespaces, Pro gate |
| `e2e_f2_polygon_anchor_dry_run_receipts` | identity+pack anchors mock, no ciphertext leak |
| `e2e_f3_hybrid_pack_sign_verify` | hybrid Ed25519 + ML-DSA commitment |
| `e2e_full_pipeline_create_to_anchor` | narrativa completa F0→F1→F3→F2 |

## % por issue (validados)

| Issue | % | Evidencia primaria |
|-------|---|--------------------|
| DL-01 F0 | 95% | node_identity 16 + persist 2 + E2E F0 |
| DL-02 F1 | 95% | challenge/namespace/pro_gate 10 + E2E F1 |
| DL-03 F2 | 90% | polygon_anchor 8 + E2E F2 (dry-run; Amoy live = ops) |
| DL-04 F3 | 100% | hybrid_pack + E2E F3 |
| DL-05 F4 | 5% | ADR only |
| DL-06 Apps | 90% | swal-node 12 (UI residual) |

## No ejecutado / fuera de alcance

- Broadcast live Amoy (`SWAL_ANCHOR_BROADCAST=1` + contrato) — requiere key funded
- WebAuthn browser E2E — requiere UI Maloca + hardware authenticator
- `cargo test -p xavier` full suite — contiene fallos ajenos previos (`health`, `sqlite_vec`, etc.)

## Reproduce

```bash
cd xavier
cargo test -p xavier --test decentralized_login_e2e --test node_fase0_persist
cargo test -p xavier --lib node_identity
cargo test -p xavier --lib polygon_anchor
cargo test -p xavier --lib challenge::
cargo test -p xavier --lib namespace::
cargo test -p xavier --lib pro_gate::
```
