# Plan: Sistema de 3 Memorias + Diagnóstico de Tests

> **Autor:** OpenClaw Subagent  
> **Fecha:** 2026-06-18  
> **Proyecto:** Xavier (`E:\cortex\xavier`)  
> **Contexto:** Sistema de memoria cognitiva para agentes AI, integrado con OpenClaw y Engram

---

## Índice

1. [Diagnóstico de Tests](#1-diagnóstico-de-tests)
2. [Arquitectura de 3 Memorias](#2-arquitectura-de-3-memorias)
3. [Pipeline de Evaluación](#3-pipeline-de-evaluación)
4. [Plan de Implementación (Fases)](#4-plan-de-implementación-fases)
5. [Comandos Específicos](#5-comandos-específicos)
6. [Apéndice: Archivos Modificados](#6-apéndice-archivos-modificados)

---

## 1. Diagnóstico de Tests

### 1.1 Error Raíz

Los 27 tests de integración fallan con el mismo error:

```
panicked at src\health\mod.rs:167:5:
can call blocking only when running on the multi-threaded runtime
```

**Causa:** La función `collect_health_sync()` en `src/health/mod.rs:167` usa:

```rust
pub fn collect_health_sync() -> HealthResponse {
    let settings = XavierSettings::current();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            collect_health_impl(&settings, None).await
        })
    })
}
```

El problema es que `collect_health_sync()` es una función síncrona que intenta ejecutar código async (`collect_health_impl`) usando `block_in_place` + `block_on`. Pero `block_in_place` requiere el **runtime multithreaded** de Tokio (`tokio::runtime::Runtime` con `threaded_scheduler`).

Los tests que fallan son de **dos tipos**:

#### Tipo A: Tests en `tests/integration.rs` (14 tests)
Usan `#[tokio::test]` que por defecto crea un `current_thread` runtime. Cuando la ruta `/health` del servidor axum llama a `collect_health_sync()`, esta función intenta `block_in_place` dentro del runtime single-threaded y **paniquea**.

Archivos afectados:
- `tests/integration.rs` — 11 tests en el bloque `mod integration`
- `tests/integration/http_api.rs` — tests individuales http
- `tests/integration/server_test.rs` — 2 tests que llaman a `/health`

#### Tipo B: Tests en `tests/integration/cli.rs` (13 tests)
Usan `#[test]` (no async, spawn proceso real). Cuando el binario xavier se inicia sin servidor corriendo, falla pero el test espera error controlado. Sin embargo `test_add_and_search_without_server` y `test_cli_subcommand_search_without_server` pueden disparar el code path que intenta usar `collect_health_sync()` al arrancar — esto produce el pánico antes del error controlado.

### 1.2 Estrategia de Fix

#### Opción 1: `#[tokio::test(flavor = "multi_thread")]` — Mínimo cambio

```rust
#[tokio::test(flavor = "multi_thread")]
async fn test_health_endpoint() {
    // ...
}
```

**Pros:** Un cambio por test, mínimo riesgo.  
**Contras:** Solución sintomática, no ataca la raíz. Tests de `server_test.rs` no pueden garantizar runtime multi-threaded.

#### Opción 2: Refactor `collect_health_sync()` — Recomendada ✅

La estrategia correcta es hacer que `collect_health_sync()` sea **segura en cualquier runtime**. Tres sub-opciones:

**2a. Detectar runtime y actuar según contexto:**

```rust
pub fn collect_health_sync() -> HealthResponse {
    let settings = XavierSettings::current();
    
    // Si ya estamos en un runtime, ejecutar directamente
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
            // Podemos usar block_in_place de forma segura
            return tokio::task::block_in_place(|| {
                handle.block_on(async {
                    collect_health_impl(&settings, None).await
                })
            });
        }
        // Runtime current_thread: crear un runtime temporal
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create temp runtime");
        return rt.block_on(async {
            collect_health_impl(&settings, None).await
        });
    }
    
    // Sin runtime: crear uno nuevo multi-threaded
    let rt = tokio::runtime::Runtime::new()
        .expect("Failed to create runtime for collect_health_sync");
    rt.block_on(async {
        collect_health_impl(&settings, None).await
    })
}
```

**2b. Runtime global lazy** (patrón usado en el proyecto para `TIME_STORE`):

```rust
use std::sync::LazyLock;

static HEALTH_RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Runtime::new().expect("Failed to create health runtime")
});

pub fn collect_health_sync() -> HealthResponse {
    let settings = XavierSettings::current();
    HEALTH_RT.block_on(async {
        collect_health_impl(&settings, None).await
    })
}
```

**2c. Hacer `health_handler` async — La más limpia:**

```rust
// En routes.rs
async fn health_handler() -> Json<serde_json::Value> {
    let health = crate::health::collect_health().await; // versión async
    // ...
}
```

Y eliminar `collect_health_sync()` por completo, usando solo `collect_health(&settings, None).await`.

### 1.3 Recomendación Final

**Usar opción 2b (Runtime Global Lazy)** por ser el mínimo cambio con solución definitiva:

1. **`src/health/mod.rs`:**
   - Añadir `LazyLock<Runtime>` global
   - Reemplazar `collect_health_sync()` para que use ese runtime
   - Eliminar `block_in_place` por completo

2. **`tests/integration.rs`:**
   - Opcional: añadir `flavor = "multi_thread"` a los `#[tokio::test]` como redundancia
   - **No obligatorio** tras el fix de `collect_health_sync()`

3. **`tests/integration/http_api.rs`:**
   - Añadir `#[tokio::test(flavor = "multi_thread")]` en los tests que lanzan servidor

4. **`tests/integration/server_test.rs`:**
   - Simplificar `test_server_start_stop` para no depender del health endpoint

### 1.4 Tests Afectados (Lista Completa)

| Archivo | Tests | Total |
|---|---|---|
| `tests/integration.rs` | `test_time_metrics_endpoint`, `test_verify_save_endpoint`, `test_session_event_endpoint`, `test_sync_check_endpoint`, `test_health_endpoint`, `test_full_memory_workflow`, `test_agent_memory_interaction`, `test_distributed_coordination` | 8 |
| `tests/integration/http_api.rs` | `test_health_endpoint`, `test_session_event_endpoint`, `test_session_event_with_injection`, `test_time_metric_endpoint`, `test_sync_check_endpoint`, `test_agent_unregister_existing`, `test_agent_unregister_missing`, `test_verify_save_without_env_vars`, `test_nonexistent_endpoint_returns_404`, `test_memory_endpoint_not_found_in_minimal_router`, `test_add_memory_not_available_in_minimal_router`, `test_memory_stats_not_available_in_minimal_router`, `test_auth_protected_endpoints_not_available_in_minimal_router`, `test_hybrid_search_not_available_in_minimal_router`, `test_reflection_not_available_in_minimal_router`, `test_concurrent_requests`, `test_multi_step_workflow` | 17 |
| `tests/integration/server_test.rs` | `test_server_start_stop`, `test_health_endpoint` | 2 |
| **Total** | | **27** |

---

## 2. Arquitectura de 3 Memorias

### 2.1 Diagrama de Convivencia

```
┌─────────────────────────────────────────────────────────────────┐
│                    OPENCLAW PIPELINE                            │
│                                                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │ Sistema A   │  │ Sistema B   │  │ Sistema C   │            │
│  │ OpenClaw    │  │ Xavier      │  │ Engram      │            │
│  │ Builtin     │  │ (Rust)      │  │ (Go)        │            │
│  │             │  │             │  │             │            │
│  │ MEMORY.md   │  │ B-tree mmry │  │ SQLite+FTS5 │            │
│  │ SQLite+FTS5 │  │ TGD/HORMER │  │ 20+ MCP     │            │
│  │ Embeddings  │  │ Mesh sync  │  │ Cloud sync  │            │
│  │ (provider)  │  │ HTTP/MCP   │  │ TUI dashbd  │            │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘            │
│         │               │               │                     │
│         ▼               ▼               ▼                     │
│  ┌──────────────────────────────────────────────────┐          │
│  │          MEMORY ROUTER (OpenClaw Skill)          │          │
│  │  - Evalúa query type y contexto                  │          │
│  │  - Decide: A, B, C, o votación                  │          │
│  │  - Cachea resultados compartidos                 │          │
│  └────────────────────────┬─────────────────────────┘          │
│                           │                                    │
│                           ▼                                    │
│  ┌──────────────────────────────────────────────────┐          │
│  │              OPENCLAW AGENT                      │          │
│  │  (cortex-memory skill via MCP Bridge)            │          │
│  └──────────────────────────────────────────────────┘          │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Puntos de Integración en OpenClaw

#### Skill `cortex-memory` (SkILL existente)
**Ubicación:** `~/clawd/agents/lasantacruz/skills/cortex-memory/SKILL.md`

Debe extenderse para soportar routing entre los 3 sistemas:

```yaml
# En SKILL.md - Routing config
memory_systems:
  builtin:
    provider: openclaw
    priority: 1  # fastest, always queried first
    query_types: [quick, ephemeral, session-only]
  xavier:
    provider: mcp  # via Xavier MCP server
    priority: 2
    query_types: [persistent, structured, semantic, tgd, mesh]
    endpoint: http://localhost:8006/mcp
  engram:
    provider: mcp  # via Engram MCP stdio
    priority: 3
    query_types: [fts, timeline, judge, compare]
    command: engram mcp
```

#### MCP Bridge (El conector universal)
OpenClaw habla MCP con servidores externos. Se configura en `claude_desktop_config.json` o equivalente:

```json
{
  "mcpServers": {
    "xavier": {
      "command": "xavier",
      "args": ["mcp"],
      "env": {
        "XAVIER_CONFIG_PATH": "E:/cortex/xavier/config/xavier.config.json"
      }
    },
    "engram": {
      "command": "engram",
      "args": ["mcp"]
    }
  }
}
```

#### Hooks de OpenClaw
Los hooks son puntos de entrada donde el router de memorias intercepta:

1. **`pre_query` hook** — Antes de responder a una búsqueda de memoria:
   - Router analiza la query
   - Decide qué sistema(s) consultar
   - Si es votación, envía a los 3 en paralelo

2. **`post_save` hook** — Después de guardar un recuerdo:
   - Save simultáneo en los 3 sistemas (modo espejo)
   - O save solo en Xavier + builtin (engram queda como backup)

3. **`consolidation` hook** — Cada N horas:
   - Sync cruzado entre los 3
   - Cache warming
   - Health check de cada sistema

### 2.3 Estrategia de Routing

| Tipo de Query | Sistema Primario | Sistema Secundario | Justificación |
|---|---|---|---|
| **Quick recall** (último contexto) | OpenClaw Builtin | Xavier | Builtin es instantáneo, Xavier da profundidad |
| **Búsqueda semántica** (embedding) | Xavier | Engram | Xavier tiene TGD + HORMER |
| **Búsqueda FTS** (texto exacto) | Engram | Xavier | FTS5 de Engram es especializado |
| **Memoria episódica** (sesiones) | Xavier | OpenClaw Builtin | Xavier tiene working memory y ACL |
| **Análisis comparativo** ("juzga" entre opciones) | Engram | — | `mem_judge` tool exclusiva de Engram |
| **Timeline de proyecto** | Engram | Xavier | `mem_timeline` tool exclusiva |
| **Mesh sync multi-nodo** | Xavier | — | Solo Xavier tiene mesh networking |
| **Auto-mejora** (TGD) | Xavier | — | Sistema propio de Xavier |
| **Cloud replication** | Engram | Xavier | Engram Cloud es opt-in |

**Regla de dedo:** Si la query necesita **contexto de sesión profundo**, usa Xavier. Si es **búsqueda rápida de texto**, usa Engram. Si es **recuerdo reciente en la misma sesión**, usa OpenClaw Builtin.

### 2.4 Modos de Operación

#### Modo Espejo (default)
Cada save → los 3 sistemas. Cada read → Builtin + Xavier (los más rápidos). Engram solo para FTS y tools exclusivas.

#### Modo Votación
Para queries críticas, los 3 sistemas responden. Un meta-scorer combina resultados. Ver sección [3. Pipeline de Evaluación](#3-pipeline-de-evaluación).

#### Modo Failover
Si Xavier falla → Engram como fallback. Si Engram falla → Builtin. Si Builtin falla → modo offline sin memoria (graceful degradation).

---

## 3. Pipeline de Evaluación

### 3.1 Cómo Enfrentar los 3 Sistemas

Cada tarea de evaluación sigue este flujo:

```
1. Definir Tarea
   ├── Query de prueba (ej: "¿qué decidimos sobre la DB migration?")
   ├── Resultado esperado (opcional, para scoring exacto)
   └── Categoría (semántica, FTS, episódica, etc.)

2. Ejecutar en los 3 Sistemas
   ├── Sistema A (OpenClaw Builtin): memory_search(query)
   ├── Sistema B (Xavier): POST /memory/search {query}
   └── Sistema C (Engram): mem_search(query) via MCP

3. Evaluar Resultados
   ├── Precisión: ¿el resultado top es relevante?
   ├── Recall: ¿encontró todo lo relevante?
   ├── Latencia: tiempo de respuesta
   └── Coste: tokens/API calls consumidos

4. Scorer Combinado
   ├── Ponderación: Precisión 40%, Recall 25%, Latencia 20%, Coste 15%
   ├── Normalización: cada métrica a 0-100
   └── Score final: weighted average

5. Registrar en MEMORIA
   ├── Fecha, tarea, sistema, score
   └── Guardar en Xavier (por supuesto) y en el benchmark log
```

### 3.2 Suite de Benchmark

Crear script `scripts/benchmark_tri_memory.sh` (o `.ps1` para Windows) que:

```bash
#!/bin/bash
# benchmark_tri_memory.sh - Evalúa 3 sistemas de memoria

BENCH_FILE="benchmark/results_$(date +%Y%m%d_%H%M).json"
QUERIES=(
  "¿qué decidimos sobre la arquitectura?"
  "cómo configurar el MCP server de Xavier"
  "última sesión de debugging del health endpoint"
  "quién es el mantenedor del proyecto"
  "issue #42 sobre FTS5"
)

for query in "${QUERIES[@]}"; do
  echo "=== Query: $query ==="

  # Sistema A: OpenClaw Builtin
  start_a=$(date +%s%N)
  # ... llamada a memory_search
  end_a=$(date +%s%N)
  echo "Builtin: $(( (end_a - start_a) / 1000000 ))ms"

  # Sistema B: Xavier MCP
  start_b=$(date +%s%N)
  # ... llamada a xavier mcp tools/call
  end_b=$(date +%s%N)
  echo "Xavier: $(( (end_b - start_b) / 1000000 ))ms"

  # Sistema C: Engram MCP
  start_c=$(date +%s%N)
  # ... llamada a engram mcp
  end_c=$(date +%s%N)
  echo "Engram: $(( (end_c - start_c) / 1000000 ))ms"

  echo "---"
done
```

### 3.3 Métricas Detalladas

| Métrica | Cómo se mide | Peso |
|---|---|---|
| **Precisión@5** | De los top-5 resultados, ¿cuántos son relevantes? (evaluación manual o por LLM) | 40% |
| **Recall** | ¿Encontró al menos el resultado más relevante esperado? | 25% |
| **Latencia p50** | Mediana de tiempo de respuesta (ms) | 20% |
| **Coste** | Tokios de embedding + tokens de respuesta | 15% (Builtin=0) |
| **Disponibilidad** | Ratio de respuestas exitosas vs errores | — (umbral, no ponderado) |

### 3.4 Sistema de Scoring / Votación

Para queries en modo **votación**, el meta-scorer combina resultados:

```rust
// Pseudocódigo del meta-scorer
pub struct VoteResult {
    pub system: String,         // "builtin" | "xavier" | "engram"
    pub content: String,
    pub relevance_score: f64,   // 0.0 - 1.0
    pub latency_ms: u64,
}

pub fn score_winner(votes: Vec<VoteResult>) -> String {
    votes
        .iter()
        .max_by(|a, b| {
            let a_score = a.relevance_score * 0.6 + (1.0 - a.latency_ms as f64 / 5000.0) * 0.4;
            let b_score = b.relevance_score * 0.6 + (1.0 - b.latency_ms as f64 / 5000.0) * 0.4;
            a_score.partial_cmp(&b_score).unwrap()
        })
        .map(|v| v.system.clone())
        .unwrap_or("xavier".into())
}
```

---

## 4. Plan de Implementación (Fases)

### Fase 1: ✅ Fix Tests de Integración (Ahora)

**Estimación:** 1-2 horas  
**Prioridad:** Crítica (27 tests fallando bloquean CI)

**Tareas:**

1. **`src/health/mod.rs`:**
   - Añadir `use std::sync::LazyLock;`
   - Añadir runtime global lazy
   - Reemplazar `collect_health_sync()` para usar el runtime global
   - Eliminar `block_in_place` por completo

2. **`tests/integration/http_api.rs`:**
   - Cambiar `#[tokio::test]` a `#[tokio::test(flavor = "multi_thread")]`

3. **`tests/integration.rs`:** (bloque `mod integration`)
   - Cambiar `#[tokio::test]` a `#[tokio::test(flavor = "multi_thread")]`

4. **`tests/integration/server_test.rs`:**
   - Ajustar o ignorar tests que dependen de servidor real

5. **Verificar:** `cargo test --test integration -- --test-threads=4`

### Fase 2: Configurar Engram como MCP Server en OpenClaw

**Estimación:** 2-3 horas  
**Prioridad:** Alta

**Tareas:**

1. **Instalar Engram:**
   ```bash
   # Windows (descarga manual)
   # Ir a https://github.com/Gentleman-Programming/engram/releases
   # Descargar engram-windows-amd64.zip
   # Extraer a C:\tools\engram\
   # Añadir al PATH
   
   # O con winget/scoop (si está disponible)
   scoop bucket add gentleman-programming https://github.com/Gentleman-Programming/scoop-bucket
   scoop install engram
   ```

2. **Inicializar base de datos Engram:**
   ```bash
   engram init
   engram doctor  # verificar integridad
   ```

3. **Conectar Engram a OpenClaw vía MCP:**
   ```json
   // En ~/.openclaw/mcp-servers.json o claude_desktop_config.json
   {
     "mcpServers": {
       "engram": {
         "command": "engram",
         "args": ["mcp"]
       }
     }
   }
   ```

4. **Verificar conexión:**
   ```bash
   engram mcp  # Inicia servidor MCP en stdio
   # Probar con:
   echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | engram mcp
   ```

5. **Extender skill `cortex-memory`:**
   - Añadir provider "engram" como sistema secundario
   - Configurar routing para FTS queries → Engram
   - Añadir `mem_judge` y `mem_timeline` como herramientas disponibles

### Fase 3: Conectar Xavier como MCP Server en OpenClaw

**Estimación:** 3-4 horas  
**Prioridad:** Alta

**Tareas:**

1. **Verificar que Xavier ya tiene MCP server:**
   - `src/server/mcp/mod.rs` ya implementa MCP endpoint vía HTTP
   - Endpoint: `GET/POST /mcp` en el router HTTP de Xavier

2. **Crear wrapper CLI para MCP stdio:**
   ```rust
   // src/main.rs o nuevo binario
   // Xavier necesita un subcomando "mcp" que inicie un servidor stdio MCP
   // que se comunique con el backend HTTP interno
   ```

3. **Configurar MCP bridge en OpenClaw:**
   ```json
   // Opción A: HTTP (si Xavier ya corre como servidor)
   {
     "mcpServers": {
       "xavier": {
         "url": "http://localhost:8006/mcp"
       }
     }
   }
   
   // Opción B: Stdio (arrancar Xavier on-demand)
   {
     "mcpServers": {
       "xavier": {
         "command": "xavier",
         "args": ["mcp"],
         "env": {
           "XAVIER_CONFIG_PATH": "E:/cortex/xavier/config/xavier.config.json"
         }
       }
     }
   }
   ```

4. **Verificar tools disponibles:**
   ```bash
   curl -X POST http://localhost:8006/mcp \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
   ```

5. **Extender tools de Xavier MCP:** Añadir más herramientas de memoria al MCP server de Xavier (actualmente solo tiene `list_projects`, `get_project_context`, `sync_gitcore` y las de `tools_memory`).

### Fase 4: Pipeline de Evaluación Tri-Memoria

**Estimación:** 4-6 horas  
**Prioridad:** Media

**Tareas:**

1. **Crear `scripts/benchmark_tri_memory.ps1`:**
   - Script PowerShell que ejecuta queries contra los 3 sistemas
   - Mide latencia, cuenta resultados, registra en JSON

2. **Crear `benchmark/` directorio:**
   - `benchmark/queries.json` — conjunto de queries de prueba
   - `benchmark/results/` — resultados históricos
   - `benchmark/scoring.rs` — módulo de scoring (puede ser standalone)

3. **Implementar meta-scorer:**
   - En Rust como parte de Xavier, o como script Python
   - Algoritmo de votación ponderada
   - Dashboard simple (HTML si no, terminal vía TUI)

4. **Registrar en Xavier:** Cada resultado de benchmark se guarda como memoria en Xavier (¡inception!).

### Fase 5: Iterar y Mejorar Xavier Basado en Resultados

**Estimación:** Continuo  
**Prioridad:** Media

**Tareas:**

1. **Analizar gaps:** ¿Dónde pierde Xavier vs Engram? (ej: FTS5, judge tool)
2. **Implementar mejoras:** Basado en los resultados del benchmark
   - Si Engram gana en FTS → mejorar FTS de Xavier
   - Si Engram gana en timeline → implementar timeline en Xavier
   - Si Engram gana en judge → implementar lógica de confrontación
3. **Mantener sync:** Solo Xavier se itera; Engram y Builtin se actualizan desde fuente original.

### Fase 6: (Futuro) Investigar App Android Flutter+Rust

**Estimación:** 2-3 semanas (investigación)  
**Prioridad:** Baja

**Tareas:**

1. **Evaluar viabilidad técnica:**
   - Flutter integración con Rust vía `flutter_rust_bridge`
   - Extraer core de Xavier (crates: `memory`, `embedding`, `search`, `storage`, sync simplificado)
   - No incluir: mesh, TGD, HORMER, enterprise features

2. **Crear POC:**
   ```bash
   # Crear proyecto Flutter
   flutter create xavier_mobile
   
   # Añadir flutter_rust_bridge
   flutter pub add flutter_rust_bridge
   
   # Configurar Rust crate
   cd native
   cargo init --lib --name xavier_core
   ```

3. **Seleccionar crates a portear:**
   - `xavier_memory` → `src/memory/`
   - `xavier_embedding` → `src/embedding/`
   - `xavier_search` → `src/search/`
   - `xavier_storage` → `src/storage/`
   - `xavier_workspace` → `src/workspace/` (simplificado)
   - Excluir: `mesh`, `consolidation`, `tgd`, `enterprise`, `tauri`

4. **Evaluar vs Engram móvil:** Engram no tiene app móvil nativa — ventaja competitiva.

---

## 5. Comandos Específicos

### 5.1 Instalar Engram

```bash
# Windows (PowerShell) — Descarga manual
# 1. Ir a https://github.com/Gentleman-Programming/engram/releases
# 2. Descargar engram-windows-amd64.zip
# 3. Extraer a C:\tools\engram
# 4. Añadir al PATH

# Alternativa con winget (si está en repo)
# winget install --id GentlemanProgramming.Engram

# O con Scoop
scoop bucket add gentleman-programming https://github.com/Gentleman-Programming/scoop-bucket
scoop install engram

# Verificar
engram version
engram doctor
```

### 5.2 Conectar Engram a OpenClaw vía MCP

```bash
# 1. Iniciar engram en modo MCP (stdin/stdout)
engram mcp

# 2. Probar tools disponibles
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | engram mcp

# 3. Probar mem_save
echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"mem_save","arguments":{"title":"test","type":"decision","what":"test engram","why":"testing mcp","where":"local","learned":"works"}}}' | engram mcp

# 4. Configurar en OpenClaw
code ~/.openclaw/mcp-servers.json
# Añadir:
# {
#   "mcpServers": {
#     "engram": {
#       "command": "engram",
#       "args": ["mcp"]
#     }
#   }
# }

# 5. Probar mem_search
echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"mem_search","arguments":{"query":"test engram","limit":5}}}' | engram mcp
```

### 5.3 Exponer Xavier como MCP Server

```bash
# Modo 1: HTTP MCP (Xavier ya corriendo como servidor)
xavier http --port 8006
# El MCP endpoint está en: POST http://localhost:8006/mcp

# Probar disponibilidad
curl http://localhost:8006/health

# Listar tools MCP
curl -X POST http://localhost:8006/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'

# Llamar una tool
curl -X POST http://localhost:8006/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}'

# Modo 2: Crear subcomando "mcp" para stdio (recomendado para OpenClaw)
# Añadir a src/main.rs:
# ─── MCP subcommand ───
# SubCommand::Mcp => {
#     let state = build_app_state().await;
#     let workspace = resolve_workspace().await;
#     // Escuchar en stdin/stdout como JSON-RPC 2.0
#     run_mcp_stdio(state, workspace).await?;
# }

# Configurar en OpenClaw
code ~/.openclaw/mcp-servers.json
# {
#   "mcpServers": {
#     "xavier": {
#       "command": "xavier",
#       "args": ["mcp"],
#       "env": {
#         "XAVIER_CONFIG_PATH": "E:/cortex/xavier/config/xavier.config.json",
#         "XAVIER_DATA_DIR": "E:/cortex/xavier/data"
#       }
#     }
#   }
# }
```

### 5.4 Scripts de Evaluación Comparativa

#### Benchmark PowerShell (`scripts/benchmark_tri_memory.ps1`)

```powershell
# benchmark_tri_memory.ps1
param(
    [string]$QueryFile = "benchmark/queries.json",
    [string]$OutputDir = "benchmark/results"
)

$Results = @()
$Queries = Get-Content $QueryFile | ConvertFrom-Json

foreach ($q in $Queries) {
    Write-Host "=== Query: $($q.text) ===" -ForegroundColor Cyan

    $result = [PSCustomObject]@{
        query = $q.text
        category = $q.category
        timestamp = (Get-Date -Format "o")
        systems = @{}
    }

    # Sistema B: Xavier HTTP
    try {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $resp = Invoke-RestMethod -Uri "http://localhost:8006/memory/search" `
            -Method Post `
            -Body (@{ query = $q.text; limit = 5 } | ConvertTo-Json) `
            -ContentType "application/json" `
            -ErrorAction Stop
        $sw.Stop()
        $result.systems.xavier = @{
            latency_ms = $sw.ElapsedMilliseconds
            results_count = $resp.results.Count
            status = "ok"
        }
    } catch {
        $result.systems.xavier = @{
            status = "error"
            error = $_.Exception.Message
        }
    }

    # Sistema C: Engram MCP via pipeline
    try {
        $mcpReq = @{
            jsonrpc = "2.0"
            id = 1
            method = "tools/call"
            params = @{
                name = "mem_search"
                arguments = @{
                    query = $q.text
                    limit = 5
                }
            }
        } | ConvertTo-Json -Compress

        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $mcpResp = $mcpReq | engram mcp 2>$null
        $sw.Stop()
        $result.systems.engram = @{
            latency_ms = $sw.ElapsedMilliseconds
            raw = $mcpResp
            status = "ok"
        }
    } catch {
        $result.systems.engram = @{
            status = "error"
            error = $_.Exception.Message
        }
    }

    $Results += $result
}

# Guardar resultados
$timestamp = Get-Date -Format "yyyyMMdd_HHmm"
$Results | ConvertTo-Json -Depth 10 | Out-File "$OutputDir/results_$timestamp.json"
Write-Host "Resultados guardados en $OutputDir/results_$timestamp.json" -ForegroundColor Green

# Mostrar resumen
$Results | ForEach-Object {
    Write-Host "`n[$($_.category)] $($_.query)" -ForegroundColor Yellow
    foreach ($sys in $_.systems.PSObject.Properties) {
        $s = $sys.Value
        if ($s.status -eq "ok") {
            Write-Host "  $($sys.Name): $($s.latency_ms)ms, $($s.results_count) resultados" -ForegroundColor Green
        } else {
            Write-Host "  $($sys.Name): ERROR - $($s.error)" -ForegroundColor Red
        }
    }
}
```

#### Benchmark Queries (`benchmark/queries.json`)

```json
[
  {
    "id": "semantic_001",
    "text": "¿qué decisión tomamos sobre la base de datos principal?",
    "category": "semantic",
    "expected": "SQLite with FTS5",
