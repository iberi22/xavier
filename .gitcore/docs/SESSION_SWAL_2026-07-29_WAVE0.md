# SESSION FINAL — Continuación SWAL (2026-07-29)

| Campo | Valor |
|-------|--------|
| **Scope** | Login residual + Registry Wave Ola 0 |
| **Login feature** | `feat-decentralized-login` **95%** (residual = ops Amoy) |
| **Ola 0** | maloca **cerrada** · veedur **cerrada en rama** (merge pendiente) |
| **Fecha** | 2026-07-29 |

---

## 1. Completado hoy

### Login (post-checkpoint 2026-07-28)

| Entrega | Evidencia |
|---------|-----------|
| Baseline revalidado | E2E 5/5 · persist 2/2 · node_identity 16/16 · polygon_anchor 8/8 |
| Foundry | v1.7.1 en `~/.foundry/bin` |
| UI WebAuthn Maloca | `3ead022` pushed — `/onboarding`, `@swal/node` 18→22 tests |
| F4 fuzzy/ZKP | **NO-GO medido** (TAR≥99% → claves 28–36 bits; zk-SABER no lattice) · DL-05/F-023 → 70% |
| Pushes | xavier `main` · maloca `main` · edge-mesh `develop` · docs/SWAL |

### Registry Wave Ola 0

| appId | DoD | Commits |
|-------|-----|---------|
| maloca | 5/8 ✅ · 3 ⚠️ | `f5317e3` `7515f42` `37387a3` on `main` (pushed) |
| veedur | 5/8 ✅ · 3 ⚠️ | `4095394` on `feat/wave0-registry-dod` (pushed) |

Regla de avance Ola 0→1: DoD ≥ 6/8 con faltantes documentados. Ambos apps en **5/8 formales + 3 stubs documentados** — los ⚠️ (#4/#5/#8) están como issues gitcore no bloqueantes. Criterio de avance **casi** listo; decidir si stubs de namespace cuentan como documentados no-bloqueantes (recomendación: **sí**, arrancar Ola 1 con issue de wiring Xavier abierto).

---

## 2. Qué falta (prioridad)

1. **Ops Amoy (login F2 → 100%)** — necesita `SWAL_ANCHOR_KEY` fondeada del usuario.
2. **Merge veedur** `feat/wave0-registry-dod` → `origin/main` (local `main` divergido: rebase/PR).
3. **Ola 1** — shelf + hosteler-ia (REGISTRY_WAVE.md).
4. Wiring Xavier real (maloca #4, veedur #4) + heartbeat live en veedur.
5. Arreglar lib `backend` veedur (LocalStore drift) y fork legacy `maloca/packages/edge-mesh`.

---

## 3. Comandos de verificación

```bash
# Login
cd xavier && cargo test -p xavier --test decentralized_login_e2e --test node_fase0_persist
cd maloca && pnpm --filter @swal/node test

# Ola 0 veedur gate
cd veedur-IA.co/backend/crates/swal-gate && cargo test
```
