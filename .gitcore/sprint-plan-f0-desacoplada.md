# Xavier Sprint Plan — Fase 0: Refuerzo Arquitectura Desacoplada

**Creado:** 2026-06-16
**Meta:** Preparar repo principal para instaladores, CI/CD, y auto-updates

## Filosofía
- Arquitectura desacoplada: cada feature es aislable
- Falla catastrófica evitable: error en 1 módulo no tumba el todo
- CI/CD robusto desde GitHub Actions para generar instaladores
- Sistema de auto-update conectado al runtime health loop

---

## 🧑 Yo (implemento rápido) — Issues concretos y acotados

| # | Issue | Lo que hay que hacer | Tamaño |
|---|-------|---------------------|--------|
| **#169** | feat-dual-license | ✅ **YA IMPLEMENTADO** en esta sesión: `src/security/license.rs`, `Config.toml`, `xavier license status/accepsi/show`, feature gate en mesh commands | ✅ HECHO |
| **#171** | [dogfood] fix compilacion | Revisar qué rompió el build del dogfood anterior y arreglarlo | 🔎 Revisar |
| **#167** | feat-runtime-health | `GET /health` endpoint, SQLite VACUUM check, embedding health, system metrics, health event bus | ⚡ MEDIO |
| **#119** | Governance mesh proposals voting | **NO lo hago** — depende de #118 (ledger) y #116 (capabilities) | ❌ Complejo |
| **#121** | Deterministic scoring engine | `src/tasks/scoring.rs` — funciones puras determinísticas con tests de regresión | ⚡ RÁPIDO |

---

## 🤖 Jules (implementa) — Trabajo pesado

| # | Issue | Por qué Jules | Prioridad |
|---|-------|--------------|-----------|
| **#118** | Signed Git-like ledger | Integración profunda con mesh, firma criptográfica, fork handling | 🔴 ALTA |
| **#116** | Capability wallet access | Biscuit tokens, verificación endpoints, revocation flow | 🔴 ALTA |
| **#117** | Data Commons collector | Consent pipeline, sanitizer, signed events, anti-leak tests | 🔴 ALTA |
| **#120** | Maintainer bounty market | Integración GitHub + mesh + rewards | 🟡 MEDIA |
| **#122** | Rust mesh stack ADR | Investigación técnica de Iroh vs libp2p | 🟡 MEDIA |
| **#124** | [Embeddings 1] Data contract | Schema, serialization, redaction tests | 🟢 BAJA |
| **#160** | AFTER_DIAGNOSTIC_PLAN | Roadmap completo producción | 🟢 BAJA |
| **#115** | [EPIC] Sovereign Mesh | Epic coordinador, no implementa directamente | 🟢 INFO |
| **#14** | E2EE Wallet encryption | AES-256-GCM / Kyber-1024 | 🔴 ALTA |

---

## 🆕 Issues nuevos a crear en GitHub

| # | Título | Label | Asignado |
|---|-------|-------|----------|
| — | feat-context-regeneration (#170 ✅ **ya existe**) | enhancement | Ninguno |
| — | feat-auto-improvement (#168 ✅ **ya existe**) | enhancement, jules | Jules |
| — | chore-ci-cd: GitHub Actions CI/CD para builds automatizados | infra, jules | Jules |
| — | chore-auto-update: Sistema de auto-update para Xavier | enhancement | Yo |
| — | test-coverage: Aumentar cobertura de tests al 80%+ | test, jules | Jules |

---

## 📋 Fases del Sprint

### 🔴 Fase 1 — Hotfix + Rápidos (Yo, hoy)
- [x] ✅ **#169** feat-dual-license (¡ya implementado en esta sesión!)
- [ ] **#171** fix dogfood build
- [ ] **#121** Deterministic scoring engine
- [ ] **#167** feat-runtime-health (GET /health, auto-VACUUM, system metrics)

### 🟡 Fase 2 — Jules issues (trabajo paralelo)
- [ ] **#118** Signed ledger (Jules)
- [ ] **#116** Capability wallet (Jules)
- [ ] **#117** Data Commons collector (Jules)
- [ ] **#14** E2EE Wallet encryption (Jules)
- [ ] **#120** Maintainer bounty (Jules)
- [ ] **#124** Embeddings data contract (Jules)
- [ ] **#122** Rust mesh ADR (Jules)

### 🟢 Fase 3 — CI/CD + Auto-update + Tests (Jules + Yo)
- [ ] **chore-ci-cd**: GitHub Actions CI/CD (Jules)
- [ ] **chore-auto-update**: Sistema de auto-update nativo (Yo)
- [ ] **test-coverage**: coverage report + Jules PRs (Jules)
- [ ] **#115** Sovereign Mesh EPIC tracking (coordinación)

### 🔵 Fase 4 — Release
- [ ] Integración PRs Jules
- [ ] Release v0.7.0 con instaladores
- [ ] Documentación de upgrade path

---

## Decisiones de Arquitectura Desacoplada

1. **Cada feature** debe poder habilitarse/deshabilitarse via feature flags
2. **Panics** en 1 módulo no afectan otros (catch_unwind + graceful degradation)
3. **Runtime health** monitorea cada subsistema por separado
4. **Dual license** como gatekeeper natural de features mesh
5. **Auto-update** como servicio independiente dentro del runtime health loop
