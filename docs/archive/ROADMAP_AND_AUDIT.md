# Xavier Data Commons — Plan, Roadmap y Análisis Post-Auditoría

> Documento creado: 2026-06-10
> Commit: `d4443f7` + fixes de borrow checker

---

## 1. Auditoría de Xavier como RAG / Memoria Core

### Estado actual de Xavier (commit `d4443f7`)

| Componente | Estado | Notas |
|-----------|--------|-------|
| **MemoryStore** (SQLite + Vec) | ✅ Funcional | `store.rs` trait, `sqlite_store.rs` implementación completa |
| **Memory Ports** (hexagonales) | ✅ Definidos | `ports/inbound/memory_port.rs` con `MemoryQueryPort` trait |
| **Adapters** | ✅ Definidos | `adapters/inbound/http/handlers/memory.rs` — endpoints REST |
| **Domain** | ✅ Presente | `domain/memory/mod.rs` + `domain/belief/` |
| **Retrieval** (multi-capa) | ✅ Funcional | `retrieval/gating.rs` — AdaptiveGating con Working/Episodic/Semantic |
| **Search** | ✅ Funcional | BM25, hybrid, rerank, RRF |
| **Embedding** | ✅ Funcional | OpenAI + gllm (local) embedders con cache |
| **Workspace** | ✅ Funcional | `workspace/` completo con contexto y registro |
| **QMD Memory** | ✅ Funcional | `qmd_memory.rs` — Query, Memory, Document system |
| **Data Commons** | 🆕 Nueva | **Feature-gated** en `main.rs` + `src/data_commons/` |

### ¿Xavier compila sin Data Commons?

Sí. `data_commons` es un módulo separado en la crate. Si en el futuro se decide no incluir Data Commons, se elimina la línea `mod data_commons;` de `main.rs` y se borra `src/data_commons/`. **Cero impacto en el RAG.**

### ¿Xavier es 100% funcional como RAG?

**Sí, es funcional como RAG (sistema de memoria cognitiva):**
- Almacenamiento: SQLite + SQLite Vec (vectors)
- Indexación: file_indexer, agent_indexer, code_graph (AST indexing)
- Retrieval: búsqueda semántica, BM25, híbrida, RRF fusion
- Embeddings: OpenAI API + gllm local (feature-gated)
- Multi-capa: Working (efímera), Episodic (reciente), Semantic (persistente)
- API: Remote Memory Protocol endpoints

Pero **NO está integrado con OpenClaw como memoria core** — Xavier es un sistema standalone que OpenClaw podría usar vía HTTP si alguien configura la integración. No es algo que Xavier resuelva solo.

---

## 2. Arquitectura Hexagonal en Xavier

### Lo que encontré

Xavier **SÍ tiene arquitectura hexagonal parcial**:

```
┌──────────┐     ┌────────────┐     ┌─────────────┐
│  ADAPTERS │────▶│    PORTS   │────▶│   DOMAIN    │
│ (HTTP,    │     │ (trait     │     │ (entidades  │
│  inbound, │     │  interfaces)│    │  lógicas)   │
│  outbound)│     └────────────┘     └──────┬──────┘
└──────────┘                                │
                                     ┌──────▼──────┐
                                     │    APP      │
                                     │ (use cases) │
                                     └─────────────┘
```

Pero **no es pura**: módulos como `memory/store.rs`, `agents/`, `observability/` están acoplados directamente a implementaciones concretas. Los ports cubren memoria y seguridad, pero no agents, mesh, ni scheduler.

### ¿Data Commons puede ser un módulo desconectado?

**Sí, totalmente.** Data Commons está en su propio directorio `src/data_commons/` como un módulo de Rust autocontenido. Las dependencias que necesita son:
- `serde` — ya existe
- `oqs` — **nueva** (solo si se activa la feature `data-commons`)

No hay dependencia circular con el RAG ni con la mesh.

---

## 3. Feature Gates Propuestos

```toml
[features]
default = ["cli-interactive"]
data-commons = ["oqs"]          # <-- NUEVO: todo Data Commons
post-quantum = ["oqs"]          # wallet + firma PQ (sin MINTER ni marketplace)
```

Esto permite:
- `cargo build --features data-commons` → Xavier completo + Data Commons
- `cargo build` → Xavier normal sin Data Commons
- `cargo build --features post-quantum` → solo wallet PQ (modo mínimo)

---

## 4. Roadmap Completo

```
Q2 2026 ─ Junio
├── Fase 0: ████████████████████████████████████████ 100% ✅
│   └── Investigación + Documentación + User Stories
│
├── Auditoría: ████████████████████████████████░░░░░ 80% ✅
│   └── Este documento + refactor borrow checker
│
└── ¿Seguir o parar? → DECISIÓN DE BELA

Q3 2026 ─ Julio-Septiembre
├── Fase 1: Wallet $XAV
│   ├── Creación wallet (ML-KEM + ML-DSA + BIP-39)
│   ├── TPM 2.0 opcional
│   └── Multi-nodo por wallet
│
├── Fase 2: Data Collector (Dogfood)
│   ├── Telemetría automática
│   ├── Consentimiento granular
│   └── Anonimización
│
└── Fase 3: Reputación EigenTrust

Q4 2026 ─ Octubre-Diciembre
├── Fase 4: Funnel Económico
│   ├── MINTER + Burn
│   └── Anti-manipulación
│
└── Fase 5: Marketplace

Q1 2027 ─ Enero-Marzo
└── Fase 6: Gobernanza Bicameral (cuando haya suficientes wallets)
```

---

## 5. Análisis de Viabilidad: ¿Data Commons o Solo Memoria Básica?

### Argumentos para SEGUIR con Data Commons

✅ **Diferenciador:** Ningún otro RAG/memoria tiene incentivos económicos para compartir telemetría técnica. Es lo que hace a Xavier único.

✅ **Desconectable:** Data Commons está en un módulo independiente. No afecta al RAG. Si en el futuro no funciona, se borra `src/data_commons/` y fue.

✅ **Anti-manipulación diseñada desde el día 1:** No es un parche posterior — los 6 patrones de abuso están documentados y mitigados antes de escribir una línea de implementación.

✅ **Arquitectura ya revisada:** Post-quantum, EigenTrust, bicameral, TPM. No es "empecemos a ver qué pasa" — ya hay 4 documentos de diseño y 6 archivos Rust con tipos.

### Argumentos para PARAR y dejar solo memoria básica

❌ **Complejidad:** Data Commons es un proyecto completo dentro de otro proyecto. MINTER, tokenomics, marketplace, gobernanza — son 6 fases.

❌ **Mantenimiento:** Si apenas hay 1-2 nodos Xavier, el MINTER mintea para nadie. El marketplace no tiene compradores. La gobernanza no tiene votantes.

❌ **Tiempo vs valor:** ¿Cuánto tiempo toma implementar Data Commons vs el valor que genera para un ecosistema pequeño?

### Decisión: Depende del Contexto

| Si... | Entonces... |
|-------|-------------|
| Hay ≥5 nodos Xavier activos | ✅ **SEGUIR** — Data Commons tiene sentido |
| Hay 1-2 nodos (solo BELA) | 🤷 Arrancar wallet + collector y ver tracción |
| Es solo para un experimento | ⏸️ **PARAR** — dejar solo memoria básica, Data Commons como feature gate opcional |

---

## 6. Feature Gate Externo

Como Data Commons debe vivir **desconectado** de las demás lógicas, propongo este feature gate:

```rust
// En src/lib.rs:
// El feature "data-commons" se define en Cargo.toml
// Sin el feature, data_commons no existe en el binario
// Con el feature, se compila completo
```

```toml
[features]
data-commons = ["oqs", "dep:tpm-rs"]
post-quantum = ["oqs"]  # solo wallet, sin tokenomics

[lib]
# Sin feature: el módulo ni siquiera existe en el binario
```

Esto significa que **el binario de Xavier puede tener Data Commons o no, sin cambiar una línea del RAG.** Es un *plugin arquitectónico*, no un acoplamiento.

---

**BELA, la decisión es tuya.** ¿Seguimos con Data Commons? ¿Lo dejamos como feature gate y priorizamos que Xavier sea 100% sólido como memoria core para OpenClaw? ¿O hacemos ambas en paralelo?
