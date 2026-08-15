# FEATURE: Decentralized Login / Node Identity (SWAL)

**Status:** `stable` | **Score:** **100%** shippable (audit 2026-08-02) | **Last Tested:** 2026-08-02  
**Plan:** `login_descentralizado_swal_mesh_a7f3c2e1` · Issues: `.gitcore/issues/login/` · Evidence: `TEST_EVIDENCE.md`

## Overview

Xavier + edge-mesh + `@swal/node` implementan login descentralizado SWAL.

**DoD shippable = Fases 0–3 en código.** Fase 4 (bio/ZKP) = research separado.  
Pro=nodo · never Stripe · mesh=data plane · Polygon=ledger (solo hashes).

## Progress real por fase

| Fase | % | Notas honestas |
|------|---|----------------|
| **F0** vault BIP39/Shamir/CLI | **100%** | Crypto+CLI+persist+brick+device_key CLI/API. UI Maloca onboarding cerrada (maloca `3ead022`) |
| **F1** mesh challenge/Pro | **100%** | Challenge+namespace+pro_gate+bridge vault |
| **F2** Polygon anchors | **100%** shippable | ABI+dry-run+live-prepared+broadcast `dao-evm`+deploy script. Amoy live tx = ops runbook (`SWAL_ANCHOR_KEY`), no code gap |
| **F3** hybrid PQ packs | **100%** | `hybrid_pack` + edge-mesh hybrid-pack; ML-KEM ADR no-go |
| **F4** bio/ZKP | **70%** | Spike + ADR (**NO-GO hot-path día 1**, watch-list) — **no cuenta** en el shippable |
| **Overall feature** | **100%** | DoD F0–F3 cerrado; Amoy live deploy fuera de DoD de código |

## Fase 0 — 95%

| ID | Estado |
|----|--------|
| DL-F0-01 | ✅ BIP39-24 + passphrase |
| DL-F0-02 | ✅ Shamir 2-of-3 (SLIP39 OOS) |
| DL-F0-03 | ✅ Vault Argon2id+AES-GCM; CLI `--device-key-hex`; `@swal/node` WebAuthn PRF API |
| DL-F0-04 | ✅ Check-codes ordenados |
| DL-F0-05 | ✅ Ed25519 + ML-DSA commitment |
| UX brick | ✅ CLI warning |
| UI Maloca | ✅ producto (maloca `3ead022`) |

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

**Baseline revalidado 2026-07-29:** mismos números — e2e 5/5 · `node_fase0_persist` 2/2 · `node_identity` 16/16 · `polygon_anchor` 8/8 (maloca `@swal/node` suite ahora 18/18 con UI WebAuthn).

Issues + %: `.gitcore/issues/login/PROGRESS.md` · Evidence: `TEST_EVIDENCE.md` · Session: `.gitcore/docs/SESSION_LOGIN_2026-07-28.md` · `.gitcore/docs/SESSION_LOGIN_2026-07-29.md`

## Paths

- `src/node_identity/`
- `src/polygon_anchor/`
- `src/mesh/{challenge,namespace,pro_gate}.rs`
- `src/cli/commands/node.rs`
- `tests/e2e/decentralized_login_e2e.rs`
- `docs/POLYGON_ANCHORS.md`
