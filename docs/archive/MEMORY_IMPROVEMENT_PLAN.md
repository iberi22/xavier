# Plan de Mejora — Sistema de Memoria de Xavier

> **Fecha:** 2026-06-22
> **Contexto:** Benchmark Tri-Memory (Xavier vs Engram vs OpenClaw)
> **Commit:** d03a162

---

## Resumen Ejecutivo

Xavier tiene **2 backends de memoria que no se comunican entre sí**:

```
memory_save → QmdMemory (ingest_typed) → docs: Vec<RwLock<MemoryDocument>>
memory_search → workspace.memory.search_filtered() → QmdMemory (store-backed)
```

**El problema:** `ingest_typed` guarda en la lista en memoria `docs`, mientras `search_filtered` consulta el `MemoryStore` (SQLite/Postgres/etc.). Ambos caminos terminan en `QmdMemory` pero no comparten el mismo dataset.

---

## Score de Madurez Actual vs Competidores

| Feature | Xavier | Engram | OpenClaw |
|---------|--------|--------|----------|
| Búsqueda híbrida (keyword + vector) | ✅ 0.883 | ✅ 0.815 | ❌ N/A |
| Multi-hop context expansion | ✅ depth 0/1/2 | ❌ No | ❌ No |
| Multi-strategy ranking (BM25 + trigram) | ❌ Solo RRF | ❌ Solo FTS5 | ✅ Full |
| Project scoping | ✅ Namespace param | ✅ Projects + tags | ❌ No |
| HTTP REST search endpoint | ❌ Solo MCP | ✅ GET /search | ❌ Solo MCP |
| Auto-captura de eventos | ❌ Manual | ❌ Manual | ✅ 26 cat. |
| Caching de resultados | ✅ search_with_cache | ❌ No | ✅ TTL 24h |
| Embeddings locales (AMD) | ✅ GLLM/wgpu | ❌ API-only | ❌ No |
| FTS5 full-text search | ✅ BM25 | ✅ FTS5 | ✅ BM25 + Porter |
| TUI interactiva | ✅ Terminal UI | ✅ TUI | ❌ No |
| Backends múltiples | ✅ 6 (file/sqlite/vec/pg/supabase/memory) | ❌ Solo SQLite | ❌ Solo SQLite |
| Almacenamiento persistente | ✅ Qmd + JSON | ✅ SQLite | ✅ SQLite |

### Puntajes estimados por feature (sobre 100%)

| Categoría | Xavier | Engram | OpenClaw |
|-----------|--------|--------|----------|
| Search/Retrieval | **75%** | 65% | 70% |
| Arquitectura | **80%** | 50% | 40% |
| Integración | 55% | **70%** | 60% |
| UX/Developer | 45% | **80%** | 60% |
| Extensibilidad | **70%** | 40% | 30% |

---

## Hallazgos Críticos

### 🔴 CRÍTICO: memory_save y memory_search no comparten datos

- `memory_save` → `workspace.ingest_typed()` → guarda en `QmdMemory.docs: Arc<RwLock<Vec<MemoryDocument>>>`
- `memory_search` → `workspace.memory.search_filtered()` → busca en el `MemoryStore` subyacente
- **Resultado:** Las 1020 Qmd memories cargadas al inicio NO son visibles por `memory_search`
- **20 memorias de benchmark guardadas con `memory_save` → invisibles**

### 🟡 ALTA: Sin endpoint HTTP directo para search

- Engram: `GET /search?q=...&limit=N` — funciona sin auth compleja
- Xavier: Solo `POST /mcp` con headers `X-Xavier-Token`, `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`
- **Problema:** Agentes externos (OpenClaw, scripts Python) no pueden consultar Xavier fácilmente

### 🟡 ALTA: Sin auto-captura de contexto

- OpenClaw memory-core captura 26 categorías automáticamente (decisiones, errores, planes)
- Xavier requiere llamadas explícitas a `memory_save`
- **Resultado:** Knowledge base incompleto sin intervención manual

### 🟢 MEDIA: Caching limitado

- Xavier ya tiene `search_with_cache_filtered()` pero no expone TTL configurable
- OpenClaw usa TTL de 24h con renovación automática
- Engram no tiene cache

### 🟢 MEDIA: FTS5 vs BM25

- Xavier usa BM25 interno (keyword search en Qmd)
- Engram usa FTS5 de SQLite
- OpenClaw usa BM25 con Porter stemming + trigram-substring
- Xavier podría beneficiarse de FTS5 directo + BM25 híbrido

---

## Plan de Implementación (Priorizado)

### Sprint 1 — Unificar backends (ALTA)

```
[memory_save] ──→ [QmdMemory.docs] ──→ memory_search ✅
                    ↓                    ↓
              [ingest_typed]        [search_filtered → MemoryStore]
                    ↓                    ↓
              docs: Vec<...>       store.search()
```

**Solución:** En `memory_search`, si `search_filtered` está vacío, hacer fallback a buscar en `QmdMemory.all_documents()`.

**Archivos a modificar:**
- `src/memory/qmd_memory.rs` — `pub fn search_filtered()` — agregar fallback a `docs`
- `src/workspace/state.rs` — Exponer `QmdMemory` en el WorkspaceContext
- `src/server/mcp/tools_memory.rs` — `memory_search` handler — usar Qmd directamente

### Sprint 2 — Endpoint HTTP REST (ALTA)

**Nuevo:** `GET /v1/memory/search?q=...&limit=N&namespace=...&depth=...`

```
GET /v1/memory/search?q=test&limit=5
→ 200 OK
→ { "results": [...], "total": 5, "latency_ms": 12.3 }
```

**Archivos a modificar:**
- `src/adapters/inbound/http/routes.rs` — agregar ruta
- `src/adapters/inbound/http/handlers/memory.rs` — nuevo handler

### Sprint 3 — Auto-captura de eventos (MEDIA)

Hook automático que guarda en memoria después de cada operación:
- Tool call → `memory_save(text="xavier:tool_called:memory_search")`
- Error → `memory_save(text="xavier:error:rate_limit_exceeded")`
- Decisión → `memory_save(text="xavier:decision:chose_sqlite_backend")`

**Categorías:** tool_call, error, decision, session_start, session_end, query_result, user_feedback

### Sprint 4 — Cache TTL configurable (MEDIA)

```rust
struct CacheConfig {
    pub ttl: Duration,          // default: 24h
    pub max_entries: usize,     // default: 1000
    pub warm_on_start: bool,    // default: true
}
```

### Sprint 5 — FTS5 directo (BAJA)

Agregar FTS5 como backend opcional adicional usando SQLite directamente, con índices FTS5 sobre el contenido de Qmd.

---

## Cómo medir el progreso

Ejecutar el benchmark después de cada sprint:

```powershell
cd E:\scripts-python\xavier
python scripts/benchmark_tri_memory.py --live --depth
```

**Métricas objetivo:**

| Sprint | Score Xavier objetivo | Score Engram | Score OpenClaw |
|--------|---------------------|--------------|----------------|
| Baseline (hoy) | 0.178 (vacío) | 0.216 | 0.000 |
| Sprint 1 | **0.600+** | 0.216 | — |
| Sprint 2 | **0.700+** | — | — |
| Sprint 3 | **0.750+** | — | — |
| Sprint 4 | **0.800+** | — | — |
| Sprint 5 | **0.850+** | — | — |

---

## Issues relacionados

- **#273** — [JULES] Investigar Engram y OpenClaw para mejorar memoria
- **#197** — feat-tri-memory-benchmark (cerrado)
- **#115** — Sovereign Mesh EPIC

---

## Resumen de cambios necesarios en el código

### 1. `src/memory/qmd_memory.rs`
```rust
// Antes: search_filtered solo busca en MemoryStore
pub async fn search_filtered(&self, query, limit, filters) -> Vec<Document> {
    self.store.search(query, limit, filters).await
}

// Despues: fallback a docs si store no encuentra
pub async fn search_filtered(&self, query, limit, filters) -> Vec<Document> {
    let results = self.store.search(query, limit, filters).await?;
    if results.is_empty() {
        // Buscar en docs directamente
        return self.search_docs(query, limit, filters);
    }
    results
}
```

### 2. `src/adapters/inbound/http/routes.rs`
```rust
.route("/v1/memory/search", get(handle_memory_search))
```

### 3. `src/adapters/inbound/http/handlers/memory.rs`
```rust
async fn handle_memory_search(
    Query(params): Query<SearchParams>,
    Extension(workspace): Extension<WorkspaceContext>,
) -> Json<SearchResponse> {
    // Buscar usando QmdMemory directamente
}
```

### 4. `src/server/mcp/tools_memory.rs`
```rust
// memory_search handler — usar QmdMemory no solo MemoryStore
```
