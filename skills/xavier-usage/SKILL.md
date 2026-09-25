---
name: xavier-usage
description: "Uso operativo de Xavier como memoria vectorial persistente: daemon REST (:8006), MCP JSON-RPC (:8100), CLI, autenticación, loop canónico search→context→save, namespaces y pitfalls. v0.14 — 2026-08-29."
version: 1.0.0
tags: [xavier, memory, mcp, rest, swaL]
---

# Xavier Usage — Skill operativa

Xavier es la memoria vectorial local (Rust + SQLite + `sqlite-vec`, BM25 + búsqueda
híbrida). Corre como daemon systemd (`xavier.service`).

## Conexión

| Superficie | Endpoint | Auth |
|-----------|----------|------|
| REST API | `http://localhost:8006` | header `X-Xavier-Token` |
| MCP JSON-RPC | `http://localhost:8100/mcp` | header `X-Xavier-Token` |
| CLI | `xavier --help` | lee `.env` automáticamente |

- Token: variable `XAVIER_TOKEN` en `apps/xavier/.env` (nunca commitear).
- `/health` responde sin auth: incluye estado de `database`, `embedding` (provider,
  modelo, dims, cache hit-rate), `llm` y `mesh`.

## Loop canónico

1. **Search** — `POST /memory/search` con `{"query": "...", "limit": 5}`.
   Devuelve candidatos con `id`, `score` y snippet.
2. **Page-in** — usa el contenido completo de los `id` ganadores
   (`/v1/memories/{id}` o MCP `memory_context`). No asumas del snippet.
3. **Save** — persiste decisiones/hallazgos con `POST /v1/memories` (o MCP
   `create_memory`). Nombres de namespace en la ruta o tags.
4. **Cita la fuente** — todo contexto recuperado se cita con su id de memoria.

## Endpoints REST principales (verificados en `src/cli/server.rs`)

| Método | Ruta | Propósito |
|--------|------|-----------|
| GET | `/health` | salud + embeddings + mesh (sin auth) |
| POST | `/memory/search` | búsqueda fat-index (query, limit, tags) |
| POST | `/v1/memories/search` | búsqueda v1 con modos `ids`/`snippet`/`full` |
| POST | `/v1/memories` | crear memoria (permiso can_add_memory) |
| GET | `/v1/memories/{id}` | page-in de una memoria |
| GET | `/v1/memories/{id}/outline` | outline de memorias largas |
| POST | `/memory/update` · `/memory/delete` | mutaciones (permissioned) |
| POST | `/memory/reindex` · `/v1/maintenance/reindex-embeddings` | reindex |
| POST | `/memory/decay` · `/memory/consolidate` · DELETE `/memory/evict` | higiene del store |
| POST | `/v1/context/assemble` | ensamblar contexto para agentes |
| GET | `/memory/stats` | conteos del store |
| POST | `/api/v1/memory/sync/push` · `/pull` · `/status` | sync mesh multi-nodo |

## MCP tools (prefijo `xavier_` cuando hay colisión)

`mem_search` (preferir sobre aliases), `memory_context` / `mem_context`,
`create_memory`, `memory_save` (texto libre con namespace), `health_check`,
`list_projects`.

## Namespaces sugeridos

- `decisions/` — decisiones de arquitectura
- `sessions/` — cierres/handoffs de sesión
- `projects/<name>/` — contexto por proyecto
- `secrets/<scope>/` — NO: no guardar tokens/secretos en memorias

## Ejemplo mínimo

```bash
TOKEN=$(grep -m1 '^XAVIER_TOKEN' .env | sed 's/^XAVIER_TOKEN=//; s/"//g')
curl -s -X POST http://localhost:8006/memory/search \
  -H "X-Xavier-Token: $TOKEN" -H 'Content-Type: application/json' \
  -d '{"query":"pendientes release readiness","limit":5}'
```

## Pitfalls

- El buscador solo encuentra memorias **con embedding**. Si el proveedor de
  embeddings falla (p. ej. OpenRouter 401), las memorias nuevas quedan invisibles:
  revisa `embedding.status` en `/health` antes de indexar en lote.
- Rate limiting activo por defecto (token bucket) desde la ola de public-readiness:
  batches agresivos reciben 429 — respeta `Retry-After`.
- `:8006/mcp/tools` está deprecado; la superficie MCP canónica es `:8100/mcp`.
- Results de search traen snippet truncado; el contenido completo requiere page-in.
