# Xavier Token Economics — Estudio de Ahorro Honesto

Este documento detalla el análisis de economía de tokens y la optimización de contexto de Xavier. Se eliminan las afirmaciones teóricas sin medición y se presenta el diseño real basado en la reducción de contexto mediante la revelación progresiva (*Progressive Disclosure*).

Este desarrollo se enmarca dentro de las iniciativas de **Ola 5 (#496)** y el protocolo de revelación progresiva y optimización de costos de MCP **(#497)**.

---

## El Problema: El Costo del Contexto Completo en Agentes LLM

En la mayoría de los flujos de trabajo con agentes AI (como Claude Code, OpenClaw o asistentes CLI tradicionales), todo el historial de la conversación y el contenido de los archivos relevantes se reenvían al LLM en cada turno. En repositorios de mediano a gran tamaño, esto provoca que el uso de tokens crezca de manera lineal o exponencial, encareciendo y ralentizando la operación del agente.

### Estimación y Ahorro Honesto de Xavier

Xavier no realiza compresión algorítmica de texto ni "magia" en el aire. El ahorro de tokens es un resultado directo de la **arquitectura de revelación progresiva** (*Progressive Disclosure*) y la granularidad de sus herramientas.

*   **Ahorro de Historial:** En lugar de enviar un historial ciego de todos los turnos, Xavier permite estructurar la memoria mediante checkpoints de conversación y budgets configurables (*Shallow*, *Medium*, *Deep*).
*   **Estimación Conservadora:** Para evitar reportes optimistas, Xavier calcula el consumo de tokens de forma honesta mediante la fórmula estándar de estimación rápida:
    $$\text{tokens} \approx \lceil \text{chars} / 4 \rceil$$
    Esta métrica es un estimador lineal conservador y transparente (ver `regression_token_estimation_honest_reporting` en el conjunto de pruebas de regresión).
*   **Ahorro por Selección Reticular:** La reducción real de tokens depende del tamaño del repositorio, la profundidad del historial y la estrategia del cliente. En un repositorio de $10\text{ MB}$, cargar todo el código insumiría más de $2.5\text{ M}$ de tokens por turno. Con Xavier, el agente solo "trae" a su contexto activo las porciones identificadas como estrictamente relevantes.

---

## El Flujo de Revelación Progresiva: `mem_search` $\rightarrow$ `memory_context`

La estrategia clave de Xavier para optimizar el uso de tokens se divide en dos fases bien definidas: **Fat Search** (Búsqueda Gorda) y **Page-In** (Paginación de Contexto Directa).

```
+-----------------------------------------------------------+
| 1. FAT SEARCH (mem_search)                                |
|    Query -> Retorna IDs, scores, paths, snippets cortos.  |
|    Uso de tokens: Mínimo (~200 - 500 tokens).             |
+-----------------------------+-----------------------------+
                              |
                              v (Agente identifica IDs clave)
+-----------------------------+-----------------------------+
| 2. PAGE-IN (memory_context)                               |
|    IDs -> Pide y concatena el contenido completo.         |
|    Uso de tokens: Acotado y bajo demanda.                 |
+-----------------------------------------------------------+
```

### Fase 1: Fat Search (`mem_search`)
El agente realiza una búsqueda híbrida (semántica + léxica) mediante la herramienta `mem_search`. Por defecto, esta consulta **no incluye el contenido completo de los documentos** (`include_content: false`).

El LLM recibe una lista de resultados con:
- `Id` (ULID único del registro)
- `Path` (Ruta o identificador del recurso)
- `Kind` (Tipo de memoria)
- `Score` (Puntuación de relevancia)
- `Snippet` (Los primeros 100 caracteres del texto como previsualización)
- `Metadata`

Esto permite al agente inspeccionar un amplio catálogo de memorias candidatas gastando apenas una fracción del contexto.

### Fase 2: Page-In (`memory_context`)
Una vez que el agente analiza las opciones devueltas por `mem_search`, selecciona los identificadores de los documentos que requiere analizar en profundidad.

A continuación, invoca la herramienta `memory_context` pasando explícitamente el parámetro `ids` con la lista de identificadores seleccionados. Esta herramienta realiza el "Page-In", recuperando el contenido completo de dichos registros desde la base de datos local de Xavier y devolviendo un bloque de contexto unificado y delimitado por `max_chars`.

---

## Ejemplos de Uso Reales (MCP y API Headless)

A continuación, se documentan los payloads exactos conformes a los esquemas de herramientas de Xavier definidos en `src/server/mcp/tools_memory.rs` y los endpoints REST de `src/server/headless/routes.rs`.

### 1. Búsqueda Fat via MCP (`mem_search`)

**Petición JSON-RPC (MCP):**
```json
{
  "jsonrpc": "2.0",
  "id": "search-1",
  "method": "tools/call",
  "params": {
    "name": "mem_search",
    "arguments": {
      "query": "autenticación oqs quantum",
      "limit": 3,
      "include_content": false
    }
  }
}
```

**Respuesta MCP (Snippet):**
```json
{
  "jsonrpc": "2.0",
  "id": "search-1",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Id: 01HZN34Y8R9WZ7K8Q5BXMC910A\nPath: docs/security/oqs_auth.md\nKind: Document\nScore: 0.9412\nSnippet: Implementación del handshake híbrido post-cuántico empleando ML-KEM y Kyber...\nMetadata: {\"tags\":[\"security\",\"crypto\"]}"
      }
    ],
    "is_error": false
  }
}
```

### 2. Page-In Context via MCP (`memory_context` / `mem_context`)

**Petición JSON-RPC (MCP):**
```json
{
  "jsonrpc": "2.0",
  "id": "context-1",
  "method": "tools/call",
  "params": {
    "name": "memory_context",
    "arguments": {
      "ids": ["01HZN34Y8R9WZ7K8Q5BXMC910A"],
      "max_chars": 4000
    }
  }
}
```

**Respuesta MCP (Snippet con contenido estructurado):**
```json
{
  "jsonrpc": "2.0",
  "id": "context-1",
  "result": {
    "content": [
      {
        "type": "structuredContent",
        "structuredContent": {
          "total_chars": 1250,
          "total_records": 1,
          "truncated": false,
          "truncated_reason": null,
          "content": "# Relevant Memory Context\n\n### docs/security/oqs_auth.md (id: 01HZN34Y8R9WZ7K8Q5BXMC910A)\nImplementación del handshake híbrido post-cuántico empleando ML-KEM y Kyber. El protocolo de intercambio de claves garantiza la confidencialidad persistente incluso frente a adversarios con capacidades de cómputo cuántico...\n",
          "sources": [
            {
              "id": "01HZN34Y8R9WZ7K8Q5BXMC910A",
              "path": "docs/security/oqs_auth.md",
              "score": 0.0,
              "snippet": "Implementación del handshake híbrido post-cuántico...",
              "provenance": {
                "source": "search_filtered",
                "retrieved_at": "2026-07-18T12:00:00Z",
                "retrieval_method": "context_depth_search",
                "embedding_model": null,
                "version": null
              },
              "metadata": {"tags":["security","crypto"]}
            }
          ]
        }
      }
    ],
    "is_error": false
  }
}
```

---

### 3. Equivalente REST Headless (Curl)

Xavier expone los endpoints correspondientes para su consumo fuera del ecosistema MCP directo.

#### Buscar con `POST /headless/memory/search` (Fat Search)

**Comando Curl:**
```bash
curl -X POST http://localhost:8006/headless/memory/search \
  -H "Content-Type: application/json" \
  -d '{
    "text": "autenticación oqs quantum",
    "limit": 3
  }'
```

**Respuesta JSON:**
```json
{
  "results": [
    {
      "id": "01HZN34Y8R9WZ7K8Q5BXMC910A",
      "path": "docs/security/oqs_auth.md",
      "revision": 1,
      "primary": true,
      "content": "Implementación del handshake híbrido post-cuántico empleando ML-KEM y Kyber. El protocolo de intercambio de claves garantiza la confidencialidad persistente incluso frente a adversarios con capacidades de cómputo cuántico...",
      "metadata": {
        "tags": ["security", "crypto"]
      }
    }
  ],
  "total": 1
}
```

*Nota:* El endpoint `/headless/memory/search` expone por defecto la estructura de `MemoryRecord` completa para integraciones HTTP simplificadas. Sin embargo, para flujos de agentes con altos requisitos de optimización de tokens, se recomienda la ruta MCP `mem_search` que reduce la transferencia de texto redundante.

#### Obtener Contexto con `GET /headless/context` (Paginación de Contexto)

**Comando Curl:**
```bash
curl -X GET "http://localhost:8006/headless/context?query=autenticacion&limit=2" \
  -H "Content-Type: application/json"
```

**Respuesta JSON:**
```json
{
  "items": [
    {
      "id": "01HZN34Y8R9WZ7K8Q5BXMC910A",
      "path": "docs/security/oqs_auth.md",
      "revision": 1,
      "primary": true,
      "content": "Implementación del handshake híbrido post-cuántico empleando ML-KEM y Kyber. El protocolo de intercambio de claves garantiza la confidencialidad persistente incluso frente a adversarios con capacidades de cómputo cuántico...",
      "metadata": {
        "tags": ["security", "crypto"]
      }
    }
  ],
  "total": 1
}
```

---

## Verificación de Integridad

El comportamiento honesto y la exactitud de estos mecanismos son evaluados continuamente mediante nuestro suite de pruebas integradas:

1.  **Prueba de Ahorro en Fat Search:** `regression_fat_search_token_savings` en `src/server/mcp/regression_token_savings.rs` verifica que las búsquedas "Fat" sin contenido mantengan un tamaño significativamente inferior al contenido real indexado.
2.  **Prueba de Page-In Dirigido:** `regression_memory_context_targeted_page_in` en el mismo módulo asegura que la invocación de `memory_context` con un array de `ids` recupere exclusivamente el contenido de esos documentos seleccionados, bloqueando la intrusión de memorias no solicitadas en el prompt.
3.  **Prueba de Estimación Honesta:** `regression_token_estimation_honest_reporting` corrobora que la estimación de tokens reportada por el sistema cumpla con la regla lineal de caracteres divididos entre 4 ($\lceil \text{chars}/4 \rceil$), previniendo reportes sesgados de eficiencia.
