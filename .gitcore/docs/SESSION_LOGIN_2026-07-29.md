# SESSION FINAL — Decentralized Login SWAL (2026-07-29)

| Campo | Valor |
|-------|--------|
| **Feature** | `feat-decentralized-login` |
| **% overall validado** | **95%** |
| **Branch Xavier** | `main` |
| **Commits sesión** | `6747f59e` (F4 spike, otro agente) + cierre docs gitcore |
| **Issues** | `.gitcore/issues/login/` |
| **Evidencia tests** | `.gitcore/issues/login/TEST_EVIDENCE.md` |

---

## 1. Objetivo de la sesión

Ejecutar research F4 (fuzzy extractor + zk-SABER) con medición real, cerrar el residual UI WebAuthn en Maloca, preparar ops Amoy (toolchain foundry) y revalidar el baseline de tests del feature.

---

## 2. Resumen de lo hecho hoy

- **Baseline revalidado (cargo test):** `decentralized_login_e2e` 5/5 · `node_fase0_persist` 2/2 · `--lib node_identity` 16/16 · `--lib polygon_anchor` 8/8.
- **F4 spike fuzzy extractor** (`docs/SWAL/spikes/fuzzy-extractor/`, 8/8 tests, Monte Carlo N=1000/config): TAR≥99% a ruido 5–10% solo con claves de 28–36 bits → helper local fuerza-brutable; FAR 0/1000 en todas las configs. zk-SABER no es lattice-based (Groth16/BN254, no-PQ, on-chain, prototipo sin auditar). Veredicto ADR: **NO-GO hot-path día 1**; watch-list con condiciones de re-apertura. DL-05: 5%→70% · F-023 (edge-mesh): 5→70.
- **UI WebAuthn Maloca (DL-06 residual UI cerrado):** `registerDeviceCredential()` en `@swal/node` + helpers base64url; 6 tests nuevos (suite 18/18 verde, `node --experimental-strip-types --test src/*.test.ts`); `OnboardingPage.svelte` en backoffice con flujos Crear (passkey → device key → tarjeta comando `XAVIER_NODE_DEVICE_KEY=… xavier node create` + checklist shares + warning brick) y Recuperar (JSON shares + `xavier node recover --shares-file … --response …`); ruta `/onboarding` + nav; typecheck/build verdes. Commit maloca `3ead022` (**pusheado**).
- **Ops Amoy (DL-03, sigue pendiente):** foundry v1.7.1 instalado en la máquina (`~/.foundry/bin`); deploy + smoke broadcast esperan la key fondeada del usuario (`SWAL_ANCHOR_KEY`). F2 sigue en 90%.

---

## 3. Commits por repo

| Repo | Commit | Estado |
|------|--------|--------|
| xavier | `6747f59e` — F4 fuzzy-extractor spike results + go/no-go [DL-05] | local (`main` +4) |
| maloca | `3ead022` — UI WebAuthn onboarding (DL-06) | **pusheado** (`acf14df..3ead022`) |
| edge-mesh | `0299e7e` — F-023 5→70 | local |
| docs/SWAL | `629924f` — spike fuzzy-extractor | local |

---

## 4. Qué falta (post-sesión)

1. Deploy Amoy + smoke broadcast con la key del usuario (`SWAL_ANCHOR_KEY`) → F2 100%
2. `git push origin main` Xavier (+4 commits locales: `d29dd9c9`, `659651f9`, `d0ffe03a`, `6747f59e`) — lo coordina el usuario
3. Sync docs → docs/SWAL monorepo

---

## 5. Comandos de verificación

```bash
cargo test -p xavier --test decentralized_login_e2e --test node_fase0_persist
cargo test -p xavier --lib node_identity
cargo test -p xavier --lib polygon_anchor
```
