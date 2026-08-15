# Graphify Integration Analysis — qué podemos tomar para Xavier

> Basado en graphify (https://github.com/safishamsi/graphify) v8
> Y Combinator S26 — herramienta de knowledge graph para AI coding assistants

## ¿Qué es Graphify?

Graphify convierte cualquier proyecto (código, docs, PDFs, imágenes, videos) en un grafo de conocimiento queryable. El usuario escribe `/graphify .` en su AI assistant y obtiene `graph.json` + `GRAPH_REPORT.md` + `graph.html`.

## Arquitectura de Graphify

```
graphify/__main__.py  →  CLI entrypoint (210 KB!)
graphify/build.py     →  Ensambla nodos+edges en NetworkX Graph
graphify/analyze.py   →  God nodes, surprising connections, suggested questions
graphify/extract.py   →  Extracción AST con tree-sitter (determinística)
graphify/cache.py     →  Cache incremental de extracciones
graphify/_minhash.py  →  MinHash para dedup de entidades
graphify/affected.py  →  Análisis de impacto (qué tocar si cambias X)
graphify/detect.py    →  Detección de tipos de archivo
```

## Ideas clave que podemos integrar en Xavier

### 1. 🏆 **`build._normalize_id()` — Normalización de IDs**
Graphify usa NFKC normalization + casefold para IDs, con un `_make_id()` que hace lo mismo. **Xavier ya tiene algo similar pero decentralizado.** Podemos unificar todo el sistema de IDs de nodos usando el mismo enfoque.

**Tomar:** El patrón de normalización (NFKC → regex \w+ → underscore collapse → casefold) garantiza que IDs generados por AST y por LLM sean reconciliables.

### 2. 🏆 **`build.build_from_json()` — Ghost node dedup**
Cuando AST y LLM extraen el mismo símbolo, Graphify detecta "ghosts" — nodos LLM que son duplicados de nodos AST canónicos. Usa `_origin=="ast"` para decidir qué nodo gana.

**Tomar:** En Xavier, cuando el NavigationPolicy + HORMER tienen edge scoring, podemos usar este patrón para deduplicar nodos entre memory layers (working vs episodic vs semantic).

### 3. 🏆 **`analyze.surprising_connections()` — Scoring compuesto**
El scoring de sorpresa usa 5 señales:
- Confidence (AMBIGUOUS > INFERRED > EXTRACTED)
- Cross file-type (code↔paper)
- Cross-repo (different top-level directory)
- Cross-community (Leiden clustering)
- Peripheral→hub connection

**Tomar:** Este scoring compuesto es directamente aplicable al `NavigationPolicy` de Xavier. En vez de solo cosine similarity + edge weight, podemos agregar cross-layer bonus, cross-directory bonus, y periférico→hub.

### 4. 🏆 **`affected.py` — Análisis de impacto (ripple effect)**
Dado un nodo, calcula qué otros nodos se verían afectados si cambia. Usa BFS en el grafo con profundidad configurable.

**Tomar:** Xavier tiene `Pathfinder` inactivo en `graph_traversal.rs`. Podemos reactivarlo para implementar `xavier nav affected <node>` — el CLI visualize que queríamos.

### 5. 🏆 **`cache.py` — Cache incremental**
Guarda resultados de extracción por archivo con hash SHA256 para evitar re-extraer. Invalida solo lo que cambió.

**Tomar:** Xavier ya tiene `qmd_memory` cache. Podemos aplicarle el mismo patrón de hash-based invalidation al TGD cache (para no re-analizar historial que no cambió).

### 6. 🏆 **`_minhash.py` — MinHash para similitud**
Usa MinHash con 128 permutaciones para detectar documentos duplicados o casi-duplicados.

**Tomar:** El `merger.rs` de Xavier usa cosine similarity. MinHash es más barato para corpus grandes y detecta documentos casi-duplicados antes de mergearlos.

### 7. 🏆 **`extract._make_id()` — File-level node IDs**
Cada archivo tiene un `_file_node_id()` que incluye el nombre del directorio padre para evitar colisiones: `cli.parser` en vez de `parser`.

**Tomar:** Xavier ya tiene directorios jerárquicos (F1). Podemos usar este mismo esquema de IDs para que el NavigationPolicy no confunda `src/memory/embed.rs` con `src/retrieval/embed.rs`.

### 8. 🏆 **`detect.py` — Extensión a language family**
Mapea extensiones de archivo a "language families" (Python, JS, Rust, etc.) para detectar edges cross-language falsos.

**Tomar:** El `merger.rs` de Xavier podría usar esto para no fusionar documentos de diferentes lenguajes a menos que haya evidencia explícita.

## Comparativa con Xavier + HORMER

| Capacidad | Graphify | Xavier + HORMER |
|-----------|----------|----------------|
| Knowledge graph | ✅ NetworkX + HTML | ✅ Graph de creencias + Qdrant |
| AST extraction | ✅ tree-sitter (9 lenguajes) | ❌ No tiene |
| Navigation scoring | ✅ Surprising connections | ✅ NavigationPolicy + GRPO |
| Entity dedup | ✅ 3-layer (AST→file→semantic) | ⚠️ Básico (solo hash) |
| CLI query | ✅ `graphify query` | ⚠️ `xavier nav` básico |
| Cache incremental | ✅ SHA256 hashing | ⚠️ Básico |
| MinHash similitud | ✅ 128 permutaciones | ❌ No tiene |
| Impact analysis | ✅ `graphify affected` | ❌ No tiene |
| File-type awareness | ✅ Language families | ❌ No tiene |
| HTML visualization | ✅ graph.html interactivo | ❌ Solo CLI |
| Multi-modal (PDF/video) | ✅ | ❌ Solo texto |

## Issues para integrar Graphify en Xavier

Basado en el análisis, propongo 6 issues para complementar los que ya tenemos:

### G1: Implementar MinHash para detección de documentos casi-duplicados
**Descripción:** Reemplazar o complementar cosine similarity en `merger.rs` con MinHash (128 permutaciones). Es más barato O(n) vs O(n²) y detecta documentos casi-duplicados que cosine similarity puede perder.
**Archivos:** `src/consolidation/merger.rs`, `src/memory/qmd/hash.rs`
**Inspiración:** `graphify/_minhash.py`

### G2: Normalización unificada de IDs de nodos
**Descripción:** Implementar `NormalizedId` en Xavier usando NFKC + casefold + underscore collapse. Usarlo en el NavigationPolicy para reconciliar IDs generados por diferentes extractores (AST vs LLM vs memory layers).
**Archivos:** `src/memory/qmd/mod.rs`, `src/retrieval/navigation.rs`
**Inspiración:** `graphify/build._normalize_id()`, `graphify/extract._make_id()`

### G3: File-type aware edge filtering
**Descripción:** Agregar un módulo de "language families" que mapee extensiones a familias. El NavigationPolicy y el merger deben ignorar edges cross-language inferidos (ej: `parse()` en Python no es la misma función que `parse()` en Rust).
**Archivos:** `src/retrieval/policy.rs`, `src/consolidation/merger.rs`
**Inspiración:** `graphify/analyze._LANG_FAMILY`, `graphify/build._cross_language()`

### G4: Scoring compuesto para NavigationPolicy
**Descripción:** Expandir el NavigationPolicy con las señales de Graphify: cross-layer bonus (working→episodic), cross-directory bonus, peripheral→hub bonus. El GRPO actual solo usa cosine similarity + edge weight.
**Archivos:** `src/retrieval/policy.rs`, `src/agents/hormer/mod.rs`
**Inspiración:** `graphify/analyze.surprising_connections()`

### G5: xavier nav affected — análisis de impacto
**Descripción:** Agregar subcomando `xavier nav affected <node>` que usa BFS para mostrar qué nodos se verían afectados si cambia un documento. Profundidad configurable (default: 2).
**Archivos:** `src/cli/commands/navigation.rs`, `src/memory/graph_traversal.rs`
**Inspiración:** `graphify/affected.py`

### G6: Cache hash-based para TGD con invalidation
**Descripción:** Agregar SHA256 hash de historial procesado para que TGD no re-analice contenido que no cambió. Similar al cache incremental de Graphify.
**Archivos:** `src/agents/tgd.rs`, `src/agents/tgd_cache.rs`
**Inspiración:** `graphify/cache.py`

## Priorización vs los issues existentes

| Prioridad | Issue | Esfuerzo | Depende de |
|-----------|-------|----------|------------|
| 🔴 Alta | G4 — Scoring compuesto | 3 días | B3 (métricas) |
| 🔴 Alta | G3 — File-type filtering | 2 días | — |
| 🟡 Media | G5 — `nav affected` | 2 días | F5 (CLI) |
| 🟡 Media | G1 — MinHash | 4 días | — |
| 🟡 Media | G6 — TGD cache | 2 días | F3 (TGD) |
| 🟢 Baja | G2 — ID normalization | 1 día | — |

## Score post-Graphify

| Capacidad | Antes | Después |
|-----------|-------|---------|
| Navigation scoring | ✅ Básico (2 señales) | ✅ Avanzado (5+ señales) |
| Entity dedup | ⚠️ Básico | ✅ Normalizado + MinHash |
| Cross-language awareness | ❌ No tiene | ✅ Language families |
| Impact analysis | ❌ No tiene | ✅ `xavier nav affected` |
| Cache inteligente | ⚠️ Básico | ✅ Hash-based incr. |
