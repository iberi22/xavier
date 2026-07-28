# SESSION FINAL — Decentralized Login SWAL (2026-07-28)

| Campo | Valor |
|-------|--------|
| **Feature** | `feat-decentralized-login` |
| **% overall validado** | **95%** |
| **Branch Xavier** | `main` |
| **Commits sesión** | `5f0d89fb` (+ docs/E2E follow-up) |
| **Issues** | `.gitcore/issues/login/` |
| **Evidencia tests** | `.gitcore/issues/login/TEST_EVIDENCE.md` |

---

## 1. Objetivo de la sesión

Analizar viabilidad del login descentralizado (seed + biometría/ZKP/PQC/mesh-chain), alinear al goal SWAL, implementar F0–F3 en Rust, documentar GitCore SRS/SRC/features, y dejar el avance guardado en Xavier.

---

## 2. Decisiones de arquitectura

| Decisión | Resultado |
|----------|-----------|
| Mesh = blockchain interna | **Rechazado** — ledger canónico = Polygon |
| Pro unlock | Nodo activo + heartbeat — **nunca Stripe** |
| Biometría/ZKP | F4 research — no hot-path día 1 |
| ML-KEM DEK | ADR **no-go día-1** |
| SLIP39 mnemonic shares | OOS (Shamir 2-of-3 cumple) |

---

## 3. Entregables por fase

### F0 — Vault local (95%)

- `src/node_identity/`: BIP39-24, Shamir, vault, check-codes, derive, persist, hybrid_pack
- CLI: `xavier node create|recover|status`
- Brick UX + `--device-key-hex`

### F1 — Mesh login (95%)

- `src/mesh/{challenge,namespace,pro_gate}.rs`
- `NodeIdentity::from_derived` / `load_preferring_swal_vault`
- Apps: `@swal/node` heartbeat + backoffice + WorldExams

### F2 — Polygon anchors (90%)

- `src/polygon_anchor/` ABI + dry-run + live-prepared + `dao-evm` broadcast
- CLI `anchor` / `anchor-pack`
- Script deploy Amoy (ops)

### F3 — Hybrid PQ (100%)

- `hybrid_pack` + ADR ML-KEM

### F4 — Research (5%)

- `ADR-SWAL-BIO-ZKP-RESEARCH.md`

---

## 4. Documentación actualizada (Xavier)

| Doc | Rol |
|-----|-----|
| `.gitcore/docs/DECENTRALIZED_LOGIN.md` | Roadmap fases |
| `.gitcore/docs/DECENTRALIZED_LOGIN_PROGRESS.md` | Changelog |
| `.gitcore/docs/LOGIN_IDENTITY_DESIGN.md` | Diseño técnico |
| `.gitcore/docs/ADR-SWAL-ML-KEM-DEK.md` | ML-KEM no-go |
| `.gitcore/docs/ADR-SWAL-BIO-ZKP-RESEARCH.md` | F4 |
| `.gitcore/features/FEATURE-feat-decentralized-login.md` | Feature 95% |
| `.gitcore/features.json` | Tracking |
| `docs/SRS/REQUIREMENTS.md` REQ-008 | SRS |
| `SRC.md` + `.gitcore/SRC.md` | SRC |
| `docs/POLYGON_ANCHORS.md` | Ops anchors |
| `.gitcore/issues/login/*` | Issues + % + evidencia |

---

## 5. Repos satélite (commits de sesión)

| Repo | Commit / nota |
|------|----------------|
| maloca | device-key + heartbeat-loop |
| edge-mesh | xavier-bridge + hybrid-pack (develop) |
| worldexams | swal-pro-heartbeat |
| docs/SWAL monorepo | filesystem (copiado a `.gitcore/docs/`) |

---

## 6. Qué falta (post-sesión)

1. Deploy Amoy + smoke `SWAL_ANCHOR_BROADCAST=1`
2. UI Maloca WebAuthn onboarding
3. Spike F4 fuzzy/ZKP (research)
4. `git push origin main` (Xavier +1 local)
5. Untracked ajenos en Xavier (issues MMR, AGENT_MAP…) — no parte de este feature

---

## 7. Comandos de verificación

```bash
cargo test -p xavier --test decentralized_login_e2e --test node_fase0_persist
cargo test -p xavier --lib node_identity
cargo test -p xavier --lib polygon_anchor
```
