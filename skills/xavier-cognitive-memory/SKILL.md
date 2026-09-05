---
name: xavier-cognitive-memory
title: Xavier Cognitive Memory & CodeGraph Protocol
description: Guía canónica para agentes de IA (Antigravity, Hermes, Jules) para consultar y persistir contexto en Xavier vía CLI, HTTP y MCP. Cubre búsqueda semántica de memorias, navegación y consultas al Code-Graph (símbolos, llamadas, blast-radius), carga on-demand lazy de bases de datos, y archivado post-ejecución.
tags:
  - xavier
  - memory
  - codegraph
  - mcp
  - orchestration
  - swal
category: orchestration
---

# Xavier Cognitive Memory & CodeGraph Protocol (SWAL)

> **MANDATORIO PARA AGENTES:** Xavier (`apps/xavier`, daemon en `:8006`, MCP en `:8100`) es la memoria central y el grafo de código unificado del ecosistema SWAL.
> Antes de escribir código, diseñar planes o refactorizar componentes, **todo agente DEBE consultar Xavier**. Al terminar una tarea, **DEBE registrar el resultado**.

---

## 1. Reglas Canónicas de Interacción

1. **Ubicación Canónica:**
   - Raíz de Xavier: `~/proyectosSWAL/apps/xavier`
   - Config / Secretos: `apps/xavier/.env` (`XAVIER_TOKEN=...`, `XAVIER_PORT=8006`)
2. **Ciclo de Vida de 3 Fases (PRE → EXEC → POST):**
   - **PRE:** Buscar decisiones arquitectónicas previas y contexto del repo.
   - **EXEC:** Consultar el Code-Graph on-demand (símbolos, ast, dependencias, blast-radius).
   - **POST:** Registrar los hallazgos y cambios (`xavier add` / `memory_save`).
3. **Carga y Descarga On-Demand:**
   - Las bases de datos de repositorios se abren en modo lazy bajo demanda.
   - Si no se usa la UI, Xavier opera en modo headless con `--no-ui`.

---

## 2. Métodos de Acceso para Agentes

### Canal A: Herramientas MCP (Model Context Protocol)
Si el agente tiene configurado el servidor MCP `xavier-memory`:

| Herramienta MCP | Uso Principal |
| :--- | :--- |
| `health_check` / `xavier_local_status` | Verifica el estado del daemon, Ollama y SQLite-vec. |
| `mem_search` / `memory_search` | Búsqueda semántica híbrida de memorias y decisiones previas. |
| `memory_save` / `save_fragment` | Guarda decisiones, soluciones o contexto en la memoria persistente. |
| `get_code_graph` | Obtiene el dump o estado del grafo de código del workspace activo. |
| `codegraph_explore` | Búsqueda de símbolos (funciones, structs, traits) por nombre o regex. |
| `trace_path` | Rastrea llamadas y dependencias entre símbolos. |

### Canal B: Interfaz de Línea de Comandos (CLI `xavier`)
Cuando se ejecutan comandos en bash o subagentes de terminal:

#### 1. Diagnóstico y Salud
```bash
# Diagnóstico completo del estado local
xavier doctor

# Estadísticas del servidor activo
xavier stats
```

#### 2. Búsqueda y Memoria Semántica
```bash
# Búsqueda semántica por query de lenguaje natural (filtrar por límite con -n)
xavier search "arquitectura de conexion sqlite" -n 5

# Agregar una nueva memoria estructurada tras concluir una tarea
xavier add "Implementada carga on-demand y flag --no-ui en Xavier" \
  --title "Modularización Xavier Wave" \
  --kind "architecture" \
  --cluster "xavier"
```

#### 3. Consultas al Code-Graph (Navegación Inteligente de Código)
El Code-Graph indexa AST con Tree-Sitter para Rust, TypeScript, Python, Go, Java, C y C++:
```bash
# Estadísticas del grafo (archivos, símbolos, lenguajes indexados)
xavier code stats

# Buscar símbolos por nombre (funciones, structs, enums)
xavier code find <NombreSimbolo>
# Ejemplo:
xavier code find ConnectionManager

# Calcular el impacto (blast-radius) de modificar un símbolo
xavier code blast-radius <NombreSimbolo>

# Ver símbolos altamente conectados (hubs arquitectónicos)
xavier code hubs

# Ver funciones o módulos con mayor complejidad ciclomática (hotspots)
xavier code hotspots

# Escanear e indexar un repositorio localmente
xavier code scan .

# Volcar y cargar el grafo portable (.xavier/codegraph.json)
xavier code dump .
xavier code load .
```

### Canal C: API REST / HTTP (`:8006`)
Para scripts automatizados o agentes HTTP:

```bash
# Cargar token desde el .env canónico
XAVIER_TOKEN="$(grep "^XAVIER_TOKEN=" ~/proyectosSWAL/apps/xavier/.env | cut -d= -f2)"

# Health check
curl -s http://localhost:8006/health

# Búsqueda de memorias
curl -s -X POST http://localhost:8006/v1/memories/search \
  -H "Authorization: Bearer $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query": "reorganización rutas swal", "limit": 5}'

# Escaneo de Code-Graph
curl -s -X POST http://localhost:8006/v1/code/scan \
  -H "Authorization: Bearer $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"path": "."}'
```

---

## 3. Protocolo Estándar de Ejecución para Agentes

```
[Inicio de Tarea]
       │
       ▼
1. ¿Existe contexto previo?
   → Ejecutar: xavier search "<término o tarea>"
       │
       ▼
2. ¿Afecta símbolos existentes en el código?
   → Ejecutar: xavier code find <Símbolo>
   → Ejecutar: xavier code blast-radius <Símbolo>
       │
       ▼
3. [Implementar cambios y ejecutar tests/validaciones]
       │
       ▼
4. Registrar memoria post-ejecución
   → Ejecutar: xavier add "<resumen del cambio y decisiones tomadas>" --kind "task_result"
```

---

## 4. Resolución de Problemas Frecuentes

- **Error: `connection refused on :8006`:**
  El daemon no está corriendo. Verificar con `systemctl --user status xavier.service` o levantarlo en modo headless:
  ```bash
  xavier http --no-ui
  ```
- **Error: `unrecognized subcommand 'query'`:**
  El subcomando correcto en el CLI para buscar símbolos es `xavier code find <simbolo>`.
- **Falta de memoria en daemon / Alto consumo:**
  Xavier descarga las bases de datos de repositorios automáticamente cada 300s mediante LRU eviction y ejecuta `PRAGMA wal_checkpoint(TRUNCATE);`.
