# 🗺️ Plan de Cierre — Code-Graph Features al 100%

> **Fecha:** 2026-07-08
> **Estado actual:** 87% (17/20 features completas, 2 parciales, 1 ausente)
> **Target:** 100% — todas las features verdes, tests pasando, clippy limpio

---

## Resumen Ejecutivo

De 21 features de code-graph, **17 están al 100%**. Quedan **4 features** con trabajo pendiente:

| Feature | % | Esfuerzo | Impacto | PR/Issue |
|---------|---|----------|---------|----------|
| **cg-ci-cd** | 0% | 🟢 ~30 min | 🔴 Crítico | — |
| **cg-watcher** | 15% | 🟡 ~2-3h | 🟡 Alto | — |
| **cg-spawn-blocking** | 0% | 🟢 ~30 min | 🟡 Medio | #464 (DRAFT) + #446 tracking |
| **cg-documentation** | 40% | 🟢 ~1h | 🟡 Medio | — |

### ⚠️ PRs Abiertos (NO Mergeados)

| PR | Estado | +/- | Conflictos | Branch |
|----|--------|-----|------------|--------|
| **#441** — MCP wiring + skill + security audit | **OPEN** | +2,033/-282 | ✅ CONFLICTOS | `feat/codegraph-mcp-wiring` (current) |
| **#464** — spawn_blocking Indexer | **DRAFT** | +20/-11 | ✅ CONFLICTOS | `fix-indexer-blocking-io-v2-...` |

### 🔒 Hallazgos de Auditoría Pendientes (#441 descubrió 9, 3 resueltos, 6 pendientes)

| # | Severidad | Hallazgo | Ubicación |
|---|-----------|----------|-----------|
| 2 | 🟠 HIGH (sec) | Token por defecto `"default-token-change-me"` | `code-graph/main.rs` |
| 3 | 🟠 HIGH (sec) | CORS `Any/Any/Any` | `code-graph/main.rs` |
| 5 | 🟠 MEDIUM | `contains_call` falsos positivos | `indexer/mod.rs:289` |
| 7 | 🟡 LOW | Divergencia rutas DB (MCP vs scanner) | `cli/config.rs` |
| 8 | 🟡 LOW | `get_code_graph` legacy solapa tools | `tools_core.rs` |
| 9 | 🟡 LOW | `QueryEngine::by_language` es stub | `query/mod.rs:264` |

> \* Funcionalidad 100% implementada pero sin tests unitarios dedicados.

---

## Sprint Único: Code-Graph 100% 🏁

### PR #441: Merge MCP Wiring + Resolver Conflictos 🔴

**Estado actual:** OPEN con conflictos. Branch `feat/codegraph-mcp-wiring` tiene +2,033 líneas. Es la branch actual de trabajo.

**Acción:**
1. Resolver conflictos de merge con main
2. Verificar: `cargo check -p xavier -p code-graph`
3. Verificar: `cargo test -p code-graph` + `cargo test -p xavier --lib server::mcp`
4. Verificar: `cargo clippy --all-targets -- -D warnings`
5. Mergear a main

**Archivos en conflicto potencial:**
- `src/server/mcp/tools_code_graph.rs`
- `code-graph/src/db/mod.rs`
- `code-graph/src/indexer/mod.rs`

---

### Feature 1: CI/CD Pipeline (0% → 100%) 🔴

**Archivos:**
- Create: `.github/workflows/code-graph-ci.yml`
- Modify: `code-graph/README.md` (badge)

**Acción:**
```yaml
name: code-graph CI
on: [push, pull_request]
jobs:
  quality:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1
      - run: cargo fmt --check --manifest-path code-graph/Cargo.toml
      - run: cargo clippy --manifest-path code-graph/Cargo.toml --all-targets -- -D warnings
      - run: cargo test --manifest-path code-graph/Cargo.toml
```

**Verificación:** `cargo test, clippy, fmt pasan en GitHub Actions`

**Criterio de éxito:** ✅ Badge verde en README, PRs bloqueados si CI falla

---

### Feature 3: File Watcher AutoSync (15% → 100%) 🟡

**Archivos:**
- Modify: `code-graph/src/indexer/watcher.rs`
- Modify: `code-graph/src/indexer/mod.rs`
- Test: Agregar tests

**Lógica:**
```
Evento Modify/Create/Remove
  → debounce 500ms
  → clear_by_file(file_path)
  → parse_file(root, &path)
  → insert_symbols() + insert_edges() — upsert por stable_id
```

**Verificación:** `cargo test` pasa, watcher modifica DB correctamente

---

### Feature 4: spawn_blocking para Indexer I/O (0% → 100%) 🟡

**PR existente:** #464 (DRAFT, +20/-11, conflicts)

**Archivos:**
- Modify: `code-graph/src/indexer/mod.rs`

**Acción:**
1. Fetch branch de Jules: `git fetch origin fix-indexer-blocking-io-v2-13303491742489367769`
2. Resolver conflictos con el estado actual del Indexer
3. Asegurar que `collect_files()` + `read_to_string()` estén en `spawn_blocking`
4. Mantener el semáforo de concurrencia (8 workers)
5. Verificar: `cargo test -p code-graph`
6. Verificar: `cargo clippy --all-targets -- -D warnings`

**Criterio de éxito:** Indexer sin I/O blocking en threads async

---

### Feature 5: Documentation (40% → 100%) 🟡

**Archivos:**
- Create: `code-graph/docs/ARCHITECTURE.md`
- Create: `code-graph/docs/API.md`
- Modify: `code-graph/README.md` (expandir)

**Contenido ARCHITECTURE.md:**
```
# code-graph Architecture

## Overview
[descripción de alto nivel]

## Layers
1. Parser Layer — tree-sitter AST → Vec<Symbol>
2. Indexer Layer — file collection → parsing → edge building → DB
3. Storage Layer — SQLite + FTS5 + edges graph
4. Query Layer — QueryEngine + QueryCache + BFS traversal
5. Server Layer — Axum HTTP sidecar + CLI

## Data Flow
[diagrama ASCII del flujo]

## Adding a New Language Parser
[guía paso a paso]
```

**Contenido API.md:**
```
# code-graph API

## HTTP Endpoints (Sidecar)
- GET /health
- POST /code/scan
- POST /code/find  
- GET /code/stats

## CLI Commands
- code-graph scan <path>
- code-graph find <query>
- code-graph stats

## Env Variables
- CODE_GRAPH_TOKEN
- CODE_GRAPH_PORT
- CODE_GRAPH_HOST
- CODE_GRAPH_DB_PATH
- CODE_GRAPH_ALLOWED_ORIGINS
```

---

### Features "100% pero sin tests directos" (opcional, P2)

#### Feature 4: trace_path tests

**Archivos:** `code-graph/src/db/cypher.rs` (agregar módulo #[cfg(test)])

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::CodeGraphDB;
    
    #[test]
    fn trace_path_forward_returns_callees() {
        let db = setup_graph_db();
        let paths = db.trace_path("symbol_a", 2, false).expect("trace");
        assert!(!paths.is_empty());
        assert!(paths.iter().any(|p| p.target_symbol == "symbol_b"));
    }
}
```

#### Feature 5: hub_nodes / hotspots tests

**Archivos:** `code-graph/src/db/mod.rs` (agregar tests)

```rust
#[test]
fn hub_nodes_returns_most_connected() {
    let db = setup_graph_db();
    let hubs = db.hub_nodes(1, 10).expect("hubs");
    assert!(!hubs.is_empty());
    assert!(hubs[0].total >= hubs[1].total);
}
```

---

## Timeline Sugerido

```
Sesión 1 (hoy):     ████████████████░░░░░░  CI/CD + Documentation
Sesión 2:           ████████████████████░░  Watcher + tests directos
Sesión 3 (buffer):  ██████████████████████  Verification final + fixes
```

**Esfuerzo total estimado:** ~3-5h distribuido en 2-3 sesiones.

---

## Verification Final (post-cierre)

```bash
# Quality gates completos
cd /mnt/e/scripts-python/xavier && \
cargo.exe fmt --manifest-path code-graph/Cargo.toml --check && \
cargo.exe clippy --manifest-path code-graph/Cargo.toml --all-targets -- -D warnings && \
cargo.exe test --manifest-path code-graph/Cargo.toml && \
cargo.exe build --manifest-path code-graph/Cargo.toml --release && \
echo "✅ ALL GATES PASSED"

# features.json validation
python3 -c "
import json
with open('code-graph/features.json') as f:
    data = json.load(f)
assert data['metadata']['overall_progress_pct'] == 100
assert data['metadata']['features_complete'] == data['metadata']['total_features']
print('✅ features.json at 100%')
"

# architecture.md updated
grep -q 'code-graph' architecture.md && echo '✅ architecture.md includes code-graph'
grep -q 'features.json' architecture.md && echo '✅ architecture.md references features.json'
```
