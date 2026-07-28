# ISSUES TREE — Login descentralizado (sesión 2026-07-28)

**Proyecto:** xavier  
**Feature:** `feat-decentralized-login` (**95%**)  
**Detalle:** [login/PROGRESS.md](./login/PROGRESS.md) · [TEST_EVIDENCE.md](./login/TEST_EVIDENCE.md)

## Árbol

### Nivel 0 (paralelo histórico F0–F3)

- **DL-01** F0 vault BIP39/Shamir — **95%** ✅
- **DL-02** F1 mesh challenge/Pro — **95%** ✅
- **DL-03** F2 Polygon anchors — **90%** ✅ (ops residual)
- **DL-04** F3 hybrid packs — **100%** ✅

### Nivel 1 (apps / research)

- **DL-06** Apps heartbeat + device_key — **90%** ✅ (UI residual) — depende F0/F1
- **DL-05** F4 bio/ZKP research — **5%** — no bloquea

## Validación

E2E `decentralized_login_e2e` **5/5** + unit **41** Xavier + **12** `@swal/node` = **53 PASS / 0 FAIL**.

## Nota ola-10 MMR

Los issues `01-feat-mmr-diversify` … `05-feat-query-classifier` en la raíz de `.gitcore/issues/` son de otra ola (search) y **no** forman parte de esta sesión de login.
