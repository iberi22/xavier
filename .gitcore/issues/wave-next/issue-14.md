# [Ola 6] feat-e2ee-wallet — Implement E2EE Wallet for Identities

> Ola 6 — Security.
> Labels: `ola6`, `wave-next`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push
- [ ] Verificar que `git show HEAD --name-only` lista LOS MISMOS archivos fuente que el título del PR describe.

## Current State (MEDIBLE)
- Feature: `feat-encryption-at-rest` handles DB encryption, but Ed25519 identity keys lack E2EE wallet storage.
- File: `src/node_identity/keys.rs` (needs wallet integration)

## Desired State (DELTA)
- **New module**: `src/crypto/wallet.rs` to handle E2EE storage of private keys.
- **Section A (node_identity/keys.rs)**: Integrate `wallet.rs` to securely load/save Ed25519 BIP39-24 keypairs.
- **New tests**: `src/crypto/wallet_tests.rs` with mock keystore scenarios.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Rust E2EE wallet secure storage patterns"
2. search: "Rust Ed25519 BIP39 secure memory zeroize"
3. search: "AES-256-GCM file encryption Rust best practices"
4. search: "Linux keyring / macOS keychain integration Rust"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Research how to securely store keys in Rust (e.g. `zeroize` crate, `keyring` crate).
2. Read `src/node_identity/keys.rs` to see how keys are generated.
3. Design an AES-256-GCM wrapper for local file wallet if OS keychain is unavailable.
4. Document findings before coding."

## Existing Code Patterns (DEBES seguir estos)
- `src/crypto/mod.rs` → AES-GCM existing patterns.
- `src/node_identity/keys.rs` → Ed25519 identity generation.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `cargo clippy --package xavier -- -D warnings` — 0 errors
- [ ] `grep -c "struct E2eeWallet" src/crypto/wallet.rs` >= 1
- [ ] `cargo test --package xavier --lib crypto::wallet` 2>&1 | grep "test result: ok"

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/crypto/wallet.rs` | None | Create file | HIGH |
| `src/crypto/mod.rs` | Exists | Export wallet | LOW |
| `src/node_identity/keys.rs` | Generates keys | Wire to wallet | MED |

## DO NOT touch (Anti-Regression)
- `src/server/*` — File Island boundary!
- NO modificar `Cargo.toml` a menos que agregues `zeroize` o `keyring` seguros.

## Anti-Hallucination Guard ⚠️
1. **READ before write**: Leer `src/crypto/mod.rs` primero.
2. **Match existing patterns**: Usar la encriptación AES ya definida.

## Verification
```bash
cargo check --package xavier
cargo test --package xavier --lib crypto::wallet
```

## Dependencies & Merge Order
- **Depends on:** None
- **Parallel with:** #218, #124, #170 (different file islands)
- **Merge order within wave:** 2
- **Expected effort:** Medium 1-4h
