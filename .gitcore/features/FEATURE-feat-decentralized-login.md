# FEATURE: Decentralized Login / Node Identity (SWAL)

**Status:** `stable` | **Score:** **95%** (E2E+unit validados 2026-07-28) | **Last Tested:** 2026-07-28  
**Plan:** `login_descentralizado_swal_mesh_a7f3c2e1` · Issues: `.gitcore/issues/login/` · Evidence: `TEST_EVIDENCE.md`

## Overview

Xavier + edge-mesh + `@swal/node` implementan login descentralizado SWAL.

**DoD shippable = Fases 0–3 en código.** Fase 4 (bio/ZKP) = research separado.  
Pro=nodo · never Stripe · mesh=data plane · Polygon=ledger (solo hashes).

## Progress real por fase (esta sesión)

| Fase | % | Notas honestas |
|------|---|----------------|
| **F0** vault BIP39/Shamir/CLI | **95%** | Crypto+CLI+persist+brick+device_key CLI/API. UI Maloca onboarding pendiente |
| **F1** mesh challenge/Pro | **95%** | Challenge+namespace+pro_gate+bridge vault. Más apps pueden adoptar heartbeat |
| **F2** Polygon anchors | **90%** | ABI+dry-run+live-prepared+broadcast `dao-evm`+script deploy. **Sin address Amoy live** (ops) |
| **F3** hybrid PQ packs | **100%** | `hybrid_pack` + edge-mesh hybrid-pack; ML-KEM ADR no-go |
| **F4** bio/ZKP | **5%** | Solo ADR research — **no cuenta** en el 95% shippable |
| **Overall feature** | **95%** | Unit tests green; residual = ops deploy + UI passkey |

## Fase 0 — 95%

| ID | Estado |
|----|--------|
| DL-F0-01 | ✅ BIP39-24 + passphrase |
| DL-F0-02 | ✅ Shamir 2-of-3 (SLIP39 OOS) |
| DL-F0-03 | ✅ Vault Argon2id+AES-GCM; CLI `--device-key-hex`; `@swal/node` WebAuthn PRF API |
| DL-F0-04 | ✅ Check-codes ordenados |
| DL-F0-05 | ✅ Ed25519 + ML-DSA commitment |
| UX brick | ✅ CLI warning |
| UI Maloca | ⬜ producto |

## Fase 1 — 95%

| ID | Estado |
|----|--------|
| DL-F1-01 | ✅ Ed25519 challenge + ML-DSA e2e edge-mesh bridge |
| DL-F1-02 | ✅ `swal/{app}/{instance}` |
| DL-F1-03 | ✅ vault → `NodeIdentity::from_derived` / ACL path |
| DL-F1-04 | ✅ `pro_gate` + `@swal/node` heartbeat loop + backoffice + WorldExams |

## Fase 2 — 90%

| ID | Estado |
|----|--------|
| DL-F2-01 | ✅ ABI + live-prepared + `dao-evm` broadcast |
| DL-F2-02 | ✅ `anchor-pack` solo content_hash |
| DL-F2-03 | ✅ receipts locales 0600 |
| Deploy Amoy | ⬜ ops (`docs/SWAL/scripts/deploy-identity-registry-amoy.sh`) |

## Fase 3 — 100%

| ID | Estado |
|----|--------|
| DL-F3-01 | ✅ hybrid Ed25519 + ML-DSA commitment |
| DL-F3-02 | ✅ ADR ML-KEM **no-go día-1** |
| DL-F3-03 | ✅ PQ path mesh auth / hybrid attach |

## Tests (validados 2026-07-28)

| Suite | Pass |
|-------|------|
| `decentralized_login_e2e` | **5/5** |
| `node_fase0_persist` | **2/2** |
| `--lib node_identity` | **16/16** |
| `--lib polygon_anchor` | **8/8** |
| challenge / namespace / pro_gate | **10/10** |
| `@swal/node` (maloca) | **12/12** |

Issues + %: `.gitcore/issues/login/PROGRESS.md` · Evidence: `TEST_EVIDENCE.md` · Session: `.gitcore/docs/SESSION_LOGIN_2026-07-28.md`

## Paths

- `src/node_identity/`
- `src/polygon_anchor/`
- `src/mesh/{challenge,namespace,pro_gate}.rs`
- `src/cli/commands/node.rs`
- `tests/e2e/decentralized_login_e2e.rs`
- `docs/POLYGON_ANCHORS.md`
