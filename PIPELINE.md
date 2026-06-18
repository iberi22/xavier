# Pipeline Xavier — Ciclo Completo de Desarrollo + Evaluación por Agentes

> **Propósito:** Pipeline CI/CD que orquesta issues → PRs → tests E2E → CI → merge → evaluación por agentes AI → feedback loop.
> **Filosofía:** Xavier es un sistema para agentes, no humanos. Los humanos tienen el chat. Los agentes consumen Xavier vía MCP + HTTP API.

---

## 🔄 Ciclo Completo

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  ISSUE   │───▶│    PR    │───▶│  TESTS   │───▶│  MERGE   │
│  Creado  │    │  Jules   │    │  E2E CI  │    │  a main  │
└──────────┘    └──────────┘    └──────────┘    └──────────┘
     ▲                                                  │
     │                                                  ▼
┌──────────┐    ┌──────────┐    ┌──────────────────────────┐
│ FEEDBACK │◀───│  REPORT  │◀───│   EVALUACIÓN POR AGENTE  │
│  Issues  │    │  Final   │    │   (uso real en OpenClaw) │
└──────────┘    └──────────┘    └──────────────────────────┘

Cada paso valida antes de pasar al siguiente.
Si algo falla → issue automático + fixes.
```

---

## Paso 1: Estado del Repo (main actualizado)

**Gate:** Siempre que se inicia un ciclo, verificar que `main` está actualizado.

```bash
cd E:\cortex\xavier
git checkout main
git fetch origin
git pull origin main
git log --oneline -5
```

**Si hay cambios locales:** commit o stash antes de continuar.

---

## Paso 2: Issues → PRs (Jules Automation)

**Regla:** Cada feature tiene su issue con label `jules`. Jules crea PR automático.

### Issues Prioritarios Actuales (18 abiertos)

| # | Título | Label | Estado |
|---|--------|-------|--------|
| 14 | E2EE Wallet & Chunk Encryption | jules | ⏳ Jules |
| 124 | Anonymous consented data export contract | jules | ⏳ Jules |
| 160 | v0.10.0 → v0.11.0 Production Roadmap | enhancement | 📋 Plan |
| 166 | Bicameral DAO on-chain integration | mesh,design | 🎯 Mesh |
| 169 | Dual License (MIT + Mesh License) | design | 📋 Legal |
| 170 | Context regeneration & perfect recall loop | jules,design | 🔧 Core |
| 193 | Phase 2 HTTP handlers for memory sync | jules | ⏳ Jules |
| 194 | libp2p transport for mesh discovery | jules | ⏳ Jules |

**Flujo:**
1. Crear issue con template feature/bug/refactor
2. Label `jules` → Jules crea PR automático
3. CI corre: `cargo check`, `cargo clippy`, `cargo test --tests`
4. Revisión por subagente (`cargo-wizard` skill)
5. Si pasa → merge a main

---

## Paso 3: Tests E2E (cobertura 100%)

### Suites actuales

| Suite | Tests | Status | Cobertura |
|-------|-------|--------|-----------|
| `cargo test --lib` (unitarios) | 888 | ✅ Pasan | Unit + core |
| `tests/mesh_security_sync_test.rs` | 14 | ✅ Pasan | Mesh E2E |
| `tests/hormer_integration.rs` | — | ⚠️ Fix aplicado | HORMER nav |
| `tests/integration.rs` (http_api, cli, server) | 27 | ❌ 27 fallaban → fix aplicado | HTTP API |

### Para llegar a cobertura 100% E2E falta:

- [ ] **Fix integración completo** — confirmar que los 27 tests pasan tras fix de `collect_health_sync`
- [ ] **Test de mesh multi-nodo real** — 2 instancias Xavier comunicándose
- [ ] **Test de TGD** — ciclo completo: save → consolidate → TGD optimize → validate
- [ ] **Test de HORMER** — navegación jerárquica con scoring RL
- [ ] **Test de MCP server** — llamadas tools/call reales via stdio y HTTP
- [ ] **Test de CLI end-to-end** — xavier init → add → search → export → sync

### Comando para ejecutar suite completa

```bash
cd E:\cortex\xavier
cargo test --tests -- --test-threads=1 --show-output 2>&1 | Tee-Object -FilePath "test_results_$(Get-Date -Format 'yyyyMMdd_HHmm').log"
```

---

## Paso 4: CI/CD Pipeline

Cada push a cualquier branch:

```yaml
# .github/workflows/ci.yml
name: Xavier CI

on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo check --workspace
      - run: cargo clippy --workspace -- -D warnings

  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --tests -- --test-threads=4

  e2e:
    runs-on: ubuntu-latest
    needs: [check, test]
    steps:
      - uses: actions/checkout@v4
      - run: |
          cargo run -- http --port 8006 &
          sleep 5
          cargo test --test integration -- --test-threads=1
          cargo test --test mesh_security_sync_test -- --test-threads=1
          curl -f http://localhost:8006/health
```

---

## Paso 5: Evaluación por Agentes (el paso más importante)

**Regla de oro:** Cada merge a main → un agente real usa Xavier durante 1 hora y reporta.

### ¿Cómo se evalúa?

1. **OpenClaw Agent** usa Xavier MCP tools en su flujo normal (`cortex-memory` skill)
2. **Métricas registradas:**
   - Latencia por tool call
   - Tasa de éxito (200 vs error)
   - Precisión de resultados (relevancia top-3)
   - Cobertura: ¿encontró lo que necesitaba?
3. **Reporte automático** se guarda en memoria de Xavier (auto-referencial)

### Script de evaluación continua

```bash
# En ~/clawd/agents/lasantacruz/skills/cortex-memory/
# Se ejecuta cada vez que el skill se activa

Memorias guardadas via Xavier: 45
Búsquedas exitosas: 38/40 (95%)
Latencia promedio: 23ms
Errores: 2 (connection refused, timeout)

→ Issues creados automáticamente:
  - fix: connection refused en startup
  - feat: aumentar timeout default a 5s
```

---

## Paso 6: Feedback Loop

```
Evaluación agente → Issues detectados → Jules fixes → PR → Tests → Merge
       ↑                                                            │
       └────────────────────────────────────────────────────────────┘
```

Cada feedback se convierte en:
- **Bug report** si algo no funciona
- **Feature request** si falta algo
- **Test request** si no hay cobertura

---

## 📊 Dashboard de Estado

```
XAVIER PIPELINE STATUS
══════════════════════

📦 RELEASE: v0.10.0 → v0.11.0 (en progreso)

✅ BUILD:     cargo check --lib → passing
✅ UNIT:      cargo test --lib → 888/888
✅ INTEG:     cargo test --tests → 135/135 — 0 FAIL
✅ MESH:      cargo test --test mesh_security_sync_test → 14/14
🔶 CI:        GitHub Actions → no configurado para este ciclo

🔴 ISSUES ABIERTOS: 23
  - #195 MCP Server para agentes
  - #196 Android APK Flutter+Rust
  - #197 Benchmark tri-memoria
  - #198 TGD consolidation
  - #199 HORMER v2
  - #194 libp2p transport
  - #193 Phase 2 HTTP
  - #200 EPIC v1.0 + 15 más

🟡 PRs ABIERTOS: 0

⚡ EVALUACIÓN POR AGENTE: No iniciada (esperando MCP Server — #195)

─────────────────────────────────────
PRÓXIMO PASO: Implementar MCP Server (#195) y empezar uso real
```

---

## 🚀 Fases de Implementación

### Fase 0 — Fundación ✅ (2026-06-18)
- [x] Fix 27 tests de integración (collect_health_sync thread-safe)
- [x] cargo check --lib pasa
- [x] Suite integración completa: 135/135
- [x] Merge fix a main y push a origin

### Fase 1 — MCP Server (ahora)
- [ ] #195 — Servidor MCP funcional para agentes
- [ ] Tools: mem_save, mem_search, mem_context, get_project_context
- [ ] Tests de integración MCP
- [ ] Evaluación por agente real (OpenClaw)

### Fase 2 — Memory Sync & Mesh
- [ ] #193 — Phase 2 HTTP handlers
- [ ] #194 — libp2p transport
- [ ] #14 — E2EE Wallet

### Fase 3 — TGD + HORMER v2
- [ ] #198 — TGD consolidation nocturna
- [ ] #199 — HORMER v2 mejoras de navegación

### Fase 4 — Integración Tri-Memoria
- [ ] #197 — Benchmark OpenClaw vs Xavier vs Engram
- [ ] CI/CD automático completo

### Fase 5 — Production Release
- [ ] #196 — Android APK (Flutter+Rust)
- [ ] #200 — EPIC v1.0 completo
- [ ] Instalar Engram como MCP server
- [ ] Xavier como MCP server en OpenClaw
- [ ] Script de benchmark comparativo
- [ ] Evaluar los 3 sistemas enfrentándolos

### Fase 3 — Iteración
- [ ] Mejorar Xavier basado en resultados de evaluación
- [ ] Issues de features faltantes
- [ ] Fixes de bugs detectados por agentes

### Fase 4 — Escalamiento
- [ ] Mesh multi-nodo con peers reales
- [ ] App Android Flutter+Rust (investigación)
- [ ] Documentación y on-boarding para otros agentes

---

## 🌐 Integración con OpenClaw (diario)

```yaml
# ~/clawd/agents/lasantacruz/skills/cortex-memory/SKILL.md
# Routing de memorias para el agente

memory_systems:
  builtin:
    provider: openclaw
    priority: 1  # rápido, siempre primero
    query_types: [quick, ephemeral, session-context]

  xavier:
    provider: mcp
    priority: 2
    query_types: [persistent, semantic, tgd, mesh, structured]
    endpoint: http://localhost:8006/mcp

  engram:
    provider: mcp
    priority: 3
    query_types: [fts, timeline, judge, benchmark]
    command: engram mcp
```

---

## 📝 Checklist del Pipeline

Cada vez que se inicia un ciclo:

- [ ] `git checkout main && git pull`
- [ ] `cargo check` pasa sin errores
- [ ] `cargo test --tests -- --test-threads=1` → 0 failures
- [ ] Issues priorizados y asignados
- [ ] PRs de Jules revisados y mergeados
- [ ] Evaluación por agente iniciada
- [ ] Resultados guardados en memoria
- [ ] Nuevos issues del feedback creados

---

*Pipeline diseñado para que Xavier evolucione mediante uso real por agentes AI, no por especificación humana.*
