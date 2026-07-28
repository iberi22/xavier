# DL-01 — F0 Vault BIP39 / Shamir / CLI

| Campo | Valor |
|-------|--------|
| **Feature** | `feat-decentralized-login` |
| **Fase** | 0 |
| **% validado** | **95%** |
| **Estado** | done (UI Maloca residual) |

## Scope

BIP39-24, Shamir 2-of-3, vault Argon2id+AES-GCM, check-codes, derive Ed25519+ML-DSA commitment, CLI `xavier node create|recover|status`, persist `$XAVIER_DATA_DIR/node/`.

## Aceptación validada

- [x] Create → vault 0600 + public identity
- [x] Recover 2-of-3 + challenge ASC/DESC → misma identidad
- [x] 1 share sola no reconstruye
- [x] Brick warning en CLI
- [x] `--device-key-hex` / env hook
- [ ] UI passkey Maloca (producto)

## Tests

| Suite | Resultado |
|-------|-----------|
| `cargo test -p xavier --lib node_identity` | PASS (16) |
| `cargo test -p xavier --test node_fase0_persist` | PASS (2) |
| `cargo test -p xavier --test decentralized_login_e2e -- e2e_f0` | PASS |

## Paths

`src/node_identity/`, `src/cli/commands/node.rs`
