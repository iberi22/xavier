# Incidente 2026-08-24: Agent Scanner duplicaba el corpus de Cursor (~42k duplicados) y el servidor Xavier quedaba colgado

**Fecha:** 2026-08-24 · **Severidad:** P0 (servicio sin `/health` ni `/v1/memories/search` durante horas)
**Fix aplicado:** commit `0cdd808e` (main, pusheado) · **Ola de seguimiento:** [Ola 17] (issues #1538-#1542, epic abierto)

---

## 1. Síntoma

- `GET /health` y `POST /v1/memories/search` en :8006 → timeout (HTTP 000), mientras `/memory/stats` respondía OK.
- Proceso `xavier-real http` a 235-380% CPU, 48 threads, 1,454 fds.
- Journal: "Auto-repair: High lag for peer xv1-... (264,000+s)" cada 30-50s (3 peers muertos desde ~3 días).

## 2. Causa raíz

**Agentic Scanner (src/memory/agent_indexer.rs) — loop de 6h en src/cli/server.rs:**

```rust
let virtual_path = format!("agent_memory://{}/{}", session.ide, uuid::Uuid::new_v4());
```

- Cada pasada (cada 6h, primer tick inmediato al arrancar) generaba un **UUID aleatorio nuevo por transcript** → `add()` (que upserta por path, store_impl.rs:153) insertaba una copia NUEVA de cada uno de los ~299 transcripts de Cursor.
- Resultado: 42,465 registros duplicados (95% de los 44,757 del vec-store) acumulados en ~25 días de operación (≈103 pasadas × 299 transcripts).
- Cada add además: reabría `code_graph.db` (~0.5s, 1.5M edges) y disparaba el auto-capture de eventos → throughput ~1-2 adds/s → la pasada saturaba el runtime durante horas → los handlers HTTP (que comparten pool/locks) dejaban de responder.
- Contribuyentes: `MemoryMax=1G` en xavier.service (QmdMemory carga ~44k docs → swap-thrash) y el loop de auto-repair del mesh sobre peers muertos.

## 3. Fixes aplicados (same day)

| Fix | Dónde | Detalle |
|-----|-------|---------|
| Dedup del scanner | `src/memory/agent_indexer.rs` | Path estable derivado del `source_file` (file_stem): mismo transcript → mismo path → add() hace UPSERT. Cero duplicados entre pasadas. |
| Env-gate del loop | `src/cli/server.rs` | `XAVIER_AGENT_SCANNER_INTERVAL` (segundos; `0` = deshabilitado). Sistema local: `scanner.conf` drop-in con `=0`. |
| RAM | `~/.config/systemd/user/xavier.service.d/memory.conf` | `MemoryMax=4G` (antes 1G). |
| Limpieza de datos | Directo en vec-store (servidor detenido) | 42,465 duplicados borrados agrupando por `**Source DB**: <ruta>` en content (patrón con asteriscos; `instr(content,'Source DB: ')` = 0 es trampa). Se conservó 1 registro por fuente, priorizando el que tenía embedding. 44,757 → 2,595. |
| Reindex embeddings | `POST /v1/maintenance/reindex-embeddings` | 2 pasadas; de 6.3% → 60% de la DB con vector real (1,660/2,747 al cierre). |
| Backfill de sesiones | `~/.hermes/scripts/backfill-xavier-2026-08-24.py` + reintentos parafraseados | 35 sesiones Hermes (18-24 ago) + 46 sesiones Antigravity indexadas; 5 rechazadas por security scanner re-enviadas como resúmenes neutros (OK). |
| Cron indexer | `hermes-session-indexer` | Reanudado (estaba pausado desde 2026-08-20 17:48 junto con los otros 24 crons). |

Verificación: `/health` responde <1s; búsqueda semántica devuelve TOP-1 correcto en 0.3-0.7s para: "perfil iberi22 empleo", "tema UI shlf", "lanzamiento red SWAL", "sesión Antigravity libSQL".

## 4. Bugs de fondo detectados → Ola 17 (issues)

| Bug | Issue | Estado |
|-----|-------|--------|
| Auto-repair mesh sin backoff (peers +3 días) + probes pesados por ciclo | #1538 | dispatch a Jules |
| Reindex sin batching (transacción gigante) + sin doble-run guard + sin progreso | #1539 | dispatch a Jules |
| code_graph.db se reabre por cada memory add (~0.5s/registro) | #1540 | dispatch a Jules |
| `embedding_status='completed'` con embedding NULL (30,563 falsos en el caso real) | #1541 | dispatch a Jules |
| "pool for memory not found" en provider in_app/webhook al arranque | #1542 | dispatch a Jules |

## 5. Números clave

| Métrica | Antes | Después |
|---------|-------|---------|
| Registros en vec-store | 44,757 | 2,747 (2,595 post-limpieza + backfill) |
| % con embedding real | 6.3% (2,837) | 60% (1,660; resto pendiente de reindex final) |
| `/health` / search | timeout (>15s) | 0.3-0.7s |
| Sesiones Hermes indexadas (última) | 2026-08-17 | 2026-08-24 (corriente) |
| Sesiones Antigravity en Xavier | 24 | 46 |
| Duplicados de Cursor | 42,465 | 299 (1 por transcript) |
| Cron indexer | pausado 20-ago | activo (cada 6h) |

## 6. Comandos útiles para el futuro

```bash
# Reindex controlado (NO lanzar dos a la vez; limit pequeño)
XT=$(tr '\0' '\n' < /proc/$(pgrep -f 'xavier-real http' | head -1)/environ | grep '^XAVIER_TOKEN=' | cut -d= -f2-)
curl -s -X POST localhost:8006/v1/maintenance/reindex-embeddings -H "X-Xavier-Token: $XT" \
  -H 'Content-Type: application/json' -d '{"dry_run": false, "limit": 500}'

# Integridad de status (post-#1541)
curl -s localhost:8006/v1/maintenance/embedding-integrity -H "X-Xavier-Token: $XT"

# Verificar embeddings reales
python3 -c "import sqlite3;c=sqlite3.connect('data/vec-store.sqlite3');print(c.execute(\"SELECT COUNT(*) FROM memory_records WHERE length(embedding)>10\").fetchone()[0])"

# Scanner: reactivar con intervalo (0 = off)
# ~/.config/systemd/user/xavier.service.d/scanner.conf → Environment=XAVIER_AGENT_SCANNER_INTERVAL=21600
```

## 7. Lecciones

1. **Un path con UUID aleatorio por pasada rompe el upsert por path** — cualquier indexador periódico debe derivar el path del contenido/fuente, no de un UUID fresco.
2. **El status `embedding_status` no refleja la realidad** — verificar SIEMPRE con `length(embedding)`, nunca confiar en el status (issue #1541).
3. **Tareas de fondo sin límite (scanner/reindex/chronicle) ahogan el HTTP server** — batching, guards anti-doble-ejecución y progreso reportable son obligatorios.
4. **`MemoryMax=1G` era insuficiente** para el modo LAZY con ~44k docs.
5. **Crons pausados en masa = memoria muerta silenciosa** — revisar periódicamente que los indexadores siguen activos.