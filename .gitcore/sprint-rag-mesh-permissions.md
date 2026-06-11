# Dev Sprint — RAG + Mesh + Permissions

**HEAD:** `72db196`  
**Inicio:** Junio 11, 2026  
**Sprint:** 1 (Primera iteración de desarrollo real)

---

## Foco del Sprint

Hacer funcional el pipeline RAG end-to-end con embeddings reales, más la base de permisos por nodo en mesh.

## Estructura de Trabajo

Cada tarea es un issue + PR. Trabajo iterativo.

---

## 🥇 Tarea 1: RAG Funcional (Embeddings)

**Problema:** El RAG pipeline existe (search/hybrid.rs + retrieval/gating.rs) pero los embeddings no jalan porque:
- `local-gllm` feature compila candle-core que tarda ~10min en build
- `OPENAI_API_KEY` está inválida
- No hay fallback automático entre providers

**Solución:** Configurar el modo `auto` para que intente local (Ollama/llama.cpp) primero, luego cloud con key válida.

### Código existente que ya funciona sin cambios:
- `src/embedding/mod.rs` — EmbedderConfig::from_env(), build_embedder()
- `src/embedding/openai.rs` — OpenAICompatibleEmbedder (funcional con key válida)
- `src/embedding/cache.rs` — CachedEmbedder con moka + SQLite
- `src/search/hybrid.rs` — HybridSearcher con keyword + vector weights
- `src/retrieval/gating.rs` — AdaptiveGating con 3 capas de memoria

**Acción concreta:**
1. Configurar `XAVIER_EMBEDDING_PROVIDER_MODE=cloud` + generar API key válida
2. Probar RAG query: `POST /memory/search?mode=hybrid`
3. Verificar que el vector search jala con la cache de embeddings

### Archivos relevantes:
```
src/embedding/
├── mod.rs       → Embedder trait, build_embedder_from_env()
├── gllm.rs      → GllmEmbedder (local, feature-gated)
├── openai.rs    → OpenAICompatibleEmbedder
└── cache.rs     → CachedEmbedder (moka LRU + SQLite)

src/search/
├── hybrid.rs    → HybridSearcher (keyword + vector)
├── bm25.rs      → BM25 full-text
├── rrf.rs       → Reciprocal Rank Fusion
├── rerank.rs    → Rerank hook
└── hooks.rs     → Hook registry

src/retrieval/
├── gating.rs    → AdaptiveGating (3 capas de memoria)
├── scoring.rs   → Scoring algorithms
└── config.rs    → Default weights
```

---

## 🥇 Tarea 2: Permisos por Nodo

**Problema:** El schema tiene `ClearanceLevel`, `MemoryNamespace`, y RBAC en enterprise/ pero no están conectados:
- Mesh transport no autoriza queries por node_id
- Memory queries no filtran por clearance del nodo remoto
- RBAC existe como enterprise feature pero no se aplica en el pipeline

**Solución:** Integrar el sistema de permisos en el flujo de retrieval mesh.

### Lo que ya existe:
- `src/memory/schema.rs` — ClearanceLevel, MemoryNamespace, MemoryQueryFilters ✅
- `src/enterprise/rbac.rs` — Role, Permission, permisos granulares ✅
- `src/domain/security/` — SecurityService, audit, error types ✅
- `src/mesh/node.rs` — NodeID con Ed25519 keypair ✅
- `src/mesh/protocol.rs` — Protocolo mesh ✅

### Lo que hay que construir:
1. `src/mesh/acl.rs` — Access Control List por NodeID
2. Integrar ClearanceLevel check en retrieval/gating.rs
3. Namespace filtering en queries mesh entrantes

### Archivos a crear/modificar:
```
src/mesh/
├── acl.rs       → [NEW] Node-level ACL
├── mod.rs       → [MODIFY] Re-export ACL
├── transport.rs → [MODIFY] Authorize requests

src/retrieval/
├── gating.rs    → [MODIFY] Clearance filter
└── scoring.rs   → [MODIFY] Score with permissions

src/enterprise/
└── rbac.rs      → [USE] Role/Permission checking
```

---

## 🥇 Tarea 3: Profundidad/Alcance en Retrieval

**Problema:** ContextZone (Atomic/Cluster/Global/Relational) y MemoryLevel (Raw/Processed/Extracted/Belief) existen en schema pero el pipeline de retrieval no los usa activamente para ajustar profundidad.

### Lo que ya existe:
- `src/memory/schema.rs` — ContextZone, MemoryLevel ✅
- `src/retrieval/gating.rs` — retrieve_layered() que soporta working/episodic/semantic ✅
- `src/retrieval/config.rs` — pesos configurados ✅

### Lo que hay que construir:
1. Mapear ContextZone → MemoryLevel en retrieval
2. Query parameter `depth` que controle qué niveles se incluyen
3. Scoring que penalice memorias fuera del alcance solicitado

### Archivos a crear/modificar:
```
src/retrieval/
├── gating.rs    → [MODIFY] Zone-aware retrieval
├── scoring.rs   → [MODIFY] Depth penalty in scoring
└── config.rs    → [MODIFY] Depth configuration

src/memory/
└── schema.rs    → [MODIFY] Helper: zone_to_levels()
```

---

## Orden de Ejecución

1. **Tarea 1 (RAG)** — Primero. Sin embeddings funcionales, no hay RAG. Configurar + validar.
2. **Tarea 3 (Profundidad)** — Segundo. Añadir control de alcance en retrieval existente.
3. **Tarea 2 (Permisos)** — Tercero. Integrar permisos en el pipeline mesh + retrieval.

Cada tarea produce un PR mergeable con tests.

---

## Pipeline de Retrieval Deseado (Post-Sprint)

```
Usuario/envía query
    ↓
[1] Embedding → vector (local gllm o cloud OpenAI)
    ↓
[2] AdaptiveGating.retrieve_layered()
    ├── Working Memory (recencia alta)
    ├── Episodic Memory (experiencias pasadas)
    └── Semantic Memory (conocimiento general)
    ↓
[3] RRF Fusion (keyword + vector + recencia)
    ↓
[4] ClearanceLevel filter (solo lo autorizado)
    ↓
[5] Zone/Depth filter (solo alcance solicitado)
    ↓
[6] Namespace isolation (solo nodo/nodo origen)
    ↓
Resultado
```
