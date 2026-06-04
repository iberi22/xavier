# Refactor Plan — Archivos Grandes

**Date:** 2026-04-19
**Objetivo:** Dividir archivos >1000 líneas en módulos más pequeños y mantenibles.

## Estado Actual

| Archivo | Líneas | Prioridad | Secciones | Esfuerzo |
|---------|--------|-----------|-----------|----------|
| `src/cli/server.rs` | 2,703 | 🔴 Crítica | ~8 | Complejo |
| `src/server/mcp_server.rs` | 2,012 | 🔴 Crítica | ~6 | Complejo |
| `src/workspace.rs` | 1,552 | 🟠 Media | ~4 | Medio |
| `src/server/http.rs` | 1,267 | 🟠 Media | ~5 | Medio |
| `src/retrieval/gating.rs` | 1,100 | 🟡 Baja | ~3 | Fácil |
| `src/cli/commands.rs` | 1,058 | 🟡 Baja | ~4 | Fácil |
| `src/coordination/message_bus.rs` | 1,035 | 🟡 Baja | ~3 | Fácil |
| `src/settings.rs` | 1,034 | 🟡 Baja | ~2 | Fácil |
| `src/memory/entity_graph.rs` | 1,017 | 🟢 Opcional | ~3 | Fácil |
| `src/memory/manager.rs` | 1,002 | 🟢 Opcional | ~3 | Fácil |

## Plan Detallado

### 1. `src/cli/server.rs` (2,703 líneas) — PRIORIDAD MÁXIMA

**Secciones identificadas:**
- Líneas 1-200: Imports + struct definitions + constants
- Líneas 201-800: CLI command handlers (start, stop, status)
- Líneas 801-1500: HTTP server setup + middleware
- Líneas 1501-2200: WebSocket handlers + session management
- Líneas 2201-2703: MCP server integration + health endpoints

**Plan:**
```
src/cli/
├── mod.rs          (re-exporta todo)
├── server.rs       (entry point ~200 lines)
├── commands.rs     (ya existe, 1058 lines — separado)
├── handlers.rs     (CLI command handlers, extraído de server.rs)
├── http_setup.rs   (configuración HTTP, middleware)
└── websocket.rs    (WebSocket handlers)
```

**Esfuerzo:** Complejo — requiere mover tipos compartidos a mod.rs y actualizar ~20 imports.

### 2. `src/server/mcp_server.rs` (2,012 líneas)

**Secciones:**
- Líneas 1-150: Imports, structs, constants
- Líneas 151-500: Tool definitions + registrations
- Líneas 501-1000: Tool implementations (core tools)
- Líneas 1001-1500: Tool implementations (search, memory)
- Líneas 1501-2012: Session management + request handling

**Plan:**
```
src/server/mcp/
├── mod.rs           (re-exporta)
├── server.rs        (~200 lines, entry + types)
├── tools_core.rs    (core tool implementations)
├── tools_memory.rs  (memory/search tools)
└── session.rs       (session management)
```

**Esfuerzo:** Complejo — refactor significativo que requiere mover impl blocks.

### 3. `src/workspace.rs` (1,552 líneas)

**Secciones:**
- Workspace struct + config
- File operations + path resolution
- Template management
- State persistence

**Plan:** Dividir en `src/workspace/` con sub-módulos.

**Esfuerzo:** Medio

### 4. `src/server/http.rs` (1,267 líneas)

**Secciones:**
- Router setup + middleware chain
- Route handlers (agrupar por prefijo: /v1/*, /api/*, /health/*)

**Plan:** Dividir handlers en archivos separados por prefijo de ruta.

**Esfuerzo:** Medio

### 5. `src/retrieval/gating.rs` (1,100 líneas)

**Secciones:**
- Adaptive retriever struct + config
- Query processing (simplificar con helper functions)
- Scoring + fusion (RRF)

**Plan:** Extraer scoring/fusion a archivo separado.

**Esfuerzo:** Fácil

## Archivos que NO refactorizar ahora

- `src/cli/commands.rs` (1,058) — está bien estructurado, cada comando es una función
- `src/coordination/message_bus.rs` (1,035) — arquitectura de event bus que ya es un solo concepto
- `src/settings.rs` (1,034) — config central, difícil de dividir sin romper imports
- `src/memory/entity_graph.rs` (1,017) / `src/memory/manager.rs` (1,002) — refactor postergable

## Acciones Inmediatas Recomendadas

1. **Hoy:** Refactorizar `src/server/mcp_server.rs` dividiendo tools en archivos separados
2. **Hoy:** Refactorizar `src/cli/server.rs` extrayendo handlers CLI
3. **Mañana:** Dividir `src/workspace.rs`
4. **Mañana:** Extraer scoring de `gating.rs`
5. **Esta semana:** `src/server/http.rs` dividir route handlers

## Riesgos

- **Error humano al mover imports** — probar con `cargo check` después de cada movimiento
- **Pérdida de contexto de git blame** — inevitable con refactors grandes
- **Conflictos con branches abiertos** — coordinar con Jules PRs antes de refactorizar
