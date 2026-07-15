# ROADMAP.md — Visión y Roadmap de Xavier

> Visión a largo plazo del proyecto Xavier — features planeados, hitos, y dirección estratégica.
> Long-term vision — planned features, milestones, and strategic direction.

---

## 🎯 Visión / Vision

**Xavier** será el sistema de memoria central para agentes de IA. No un vector DB tradicional — un **context engine** que permite a los agentes recordar, razonar, mejorar y coordinarse.

```
Meta: Que cualquier agente AI (OpenClaw, codex, Jules, etc.)
      use Xavier como su único cerebro persistente.
```

---

## 📊 Estado Actual / Current Status

**Release:** v0.12.0
**Maturity:** 72% reconciled (20 features, 14 stable, 10 complete)
**Último sprint:** Jul 2026 — Code Graph MCP wiring, dual license hardening, auto-improvement

---

## 🗺️ Hitos / Milestones

### ✅ Hito 0: Fundación (2026-03 — 2026-05)
- [x] SQLite + SQLite-Vec storage
- [x] BM25 + Vector hybrid search
- [x] HTTP API + CLI
- [x] Encryption at rest (AES-256-GCM + Argon2)
- [x] Session management
- [x] Belief graph

### ✅ Hito 1: Integración de Agentes (2026-05 — 2026-06)
- [x] MCP Server (12 tools)
- [x] Code Graph index + structural tools
- [x] HORMER hierarchical navigation
- [x] Telegram bot integration
- [x] OpenClaw agent scanner/indexer
- [x] Subagent dispatcher (AGI + Jules)

### ✅ Hito 2: Enterprise Core (2026-06 — 2026-07)
- [x] Runtime health & self-monitoring
- [x] Notification system + event bus
- [x] Dual license (AGPL + Commercial)
- [x] Auto-Improvement Loop (Phase 1)
- [x] Context Regeneration (Phase 1)
- [x] Governance DAO (XIP lifecycle + weighted voting)
- [x] Documentation site (Starlight)

### 🏗️ Hito 3: Malla y Gobernanza (2026-07 — 2026-08)
- [ ] **P2P Mesh Real** — Iroh/QUIC transport, NAT traversal, peer discovery
- [ ] **Loro CRDT** — Conflict-free merge para sync offline
- [ ] **Governance DAO on-chain** — Solana/Ethereum integration
- [ ] **Data Commons reward funnel** — EigenTrust reputation + marketplace
- [ ] **Auto-Improvement Loop (Phase 2)** — Full CI integration
- [ ] **Context Regeneration → Perfect Recall** — recall@k → 100%

### 🎯 Hito 4: Escalamiento (2026-08 — 2026-09)
- [ ] **Panel UI estable** — Tauri desktop app full-featured
- [ ] **Multi-nodo mesh** — 3+ peers sincronizados
- [ ] **Enterprise features** — SSO, audit logging, RBAC
- [ ] **Benchmark suite** — Regresión automática en CI
- [ ] **v1.0 Release Candidate**

### 🚀 Hito 5: Producción (2026-09 — 2026-10)
- [ ] **Xavier v1.0** — Estable, documentado, probado
- [ ] **Mesh público** — Nodos federados
- [ ] **XIP activo** — Gobernanza comunitaria funcionando
- [ ] **Data Commons activo** — Contribuciones anónimas

---

## 📋 Features Planeados / Planned Features

### Core Engine
| Feature | Prioridad | Estado | Issue |
|---------|-----------|--------|-------|
| Context Regeneration & Perfect Recall | 🔴 Alta | 60% (Phase 2) | — |
| Auto-Improvement Loop (full cycle) | 🔴 Alta | 85% (Phase 1) | — |
| Memory tiers (L0/L1/L2) | 🟡 Media | Diseño | — |
| Structured memories (typed fields) | 🟡 Media | Diseño | — |
| Memory compression/summarization | 🟡 Media | Ideas | — |

### Mesh & Network
| Feature | Prioridad | Estado | Issue |
|---------|-----------|--------|-------|
| Iroh/QUIC transport | 🔴 Alta | 0% | #115 |
| Loro CRDT merge | 🟡 Media | 0% | #115 |
| NAT traversal | 🟡 Media | 0% | #115 |
| Tor/Yggdrasil transport | 🟢 Baja | 0% | — |

### Governance
| Feature | Prioridad | Estado | Issue |
|---------|-----------|--------|-------|
| DAO on-chain (Solana/EVM) | 🔴 Alta | 0% | #166 |
| Quorum detection | 🟡 Media | 0% | — |
| UI para proposals/voting | 🟡 Media | 0% | — |

### Data Commons
| Feature | Prioridad | Estado | Issue |
|---------|-----------|--------|-------|
| EigenTrust reputation | 🟡 Media | 0% | — |
| Marketplace | 🟢 Baja | 0% | — |
| Post-quantum encryption | 🟢 Baja | Diseño | — |

---

## 🧪 Experimentos / Experiments

| Experimento | Hipótesis | Estado |
|-------------|-----------|--------|
| HORMER vs flat search | Navegación jerárquica es 2x más rápida | ✅ Validado |
| RRF auto-tuning | Pesos dinámicos mejoran recall@k en 15% | 🏗️ En progreso |
| Benchmark-driven improvement | Ciclo cerrado mejora recall 5%/semana | 🏗️ En progreso |

---

## 📈 KPIs / Key Performance Indicators

| KPI | Actual | Objetivo v1.0 |
|-----|--------|---------------|
| recall@k (top-5) | ~85% | 95%+ |
| Latencia búsqueda | ~23ms | <10ms |
| Tests pasando | ~943 | 1000+ |
| Features estables | 14/20 | 20/20 |
| Cobertura de código | ~65% | 80%+ |
| Documentación alineada | ~85% | 100% |

---

## 🔄 Ciclo de Release / Release Cycle

```
Cada 2-4 semanas:
1. Definir objetivos del sprint
2. Issues → Jules → PR → CI → Merge
3. Evaluación por agente real
4. Feedback → nuevos issues
5. Release tag
```

---

## 🧭 Dirección Estratégica / Strategic Direction

### Corto Plazo (Jul-Ago 2026)
- Cerrar gaps de features (Context Regen, Auto-Improvement)
- Mesh real con Iroh/QUIC
- Governance DAO on-chain

### Mediano Plazo (Sep-Oct 2026)
- Xavier v1.0 estable
- Panel UI para humanos
- Mesh público federado

### Largo Plazo (2027+)
- Xavier como protocolo abierto de memoria
- Data Commons como marketplace de datos para entrenamiento
- Gobernanza descentralizada vía XIPs

---

_Construyendo el cerebro persistente para la próxima generación de agentes AI._
_Última actualización: 2026-07-09_
