# CodeGraph git sync

Xavier puede actualizar el grafo de código de forma incremental a partir de
`git diff`, sin re-escanear todo el árbol archivo por archivo.

## Flujo

```
git diff --name-status → paths afectadas → AST reparse → patch símbolos/edges → dump JSON
         └─ (opcional --memory) → upsert cards en /memory/add  path=code/{repo}/{stable_id}
```

No usa embeddings por carácter: solo AST + edges (igual que `xavier code scan`).
Los `stable_id` son **estructurales (v2)**: `project|file|name|kind|parent|signature`
(sin `start_line`), así un move intra-archivo no rompe edges ni memoria.

## Uso

```bash
# Primera vez / grafo vacío: hace un full scan de `.` y guarda checkpoint HEAD
xavier code sync --git

# Incremental vs último checkpoint (.xavier/codegraph-sync-commit)
xavier code sync --git

# Base explícita
xavier code sync --git --base HEAD~3

# Solo staged
xavier code sync --git --staged

# También publicar resúmenes de símbolos en memoria Xavier (cap 80)
xavier code sync --git --memory
```

También vía HTTP (servidor en marcha): `POST /code/sync` con
`{"git":true,"base":null,"staged":false,"memory":false}`.

## Checkpoint

Archivo: `.xavier/codegraph-sync-commit`  
Contiene el SHA de `HEAD` tras un sync exitoso. Si no existe y el grafo ya
tiene símbolos, el default es `HEAD~1`.

## Hook opcional (post-commit)

No se instala automáticamente. Instalación idempotente:

```bash
bash scripts/hooks/install-post-commit-codegraph.sh
```

O manualmente:

```bash
ln -sf ../../scripts/hooks/post-commit-codegraph.sh .git/hooks/post-commit
```

El hook hace soft-fail: un error de sync no bloquea el commit.

## Doctor

`xavier doctor` incluye un check soft **CodeGraph Index**:
`total_symbols == 0` → Warn (exit code sigue siendo 0 si el resto está OK).

`GET /code/stats` también marca `"degraded": true` cuando el grafo está vacío.

## Limitaciones conocidas

- Tras actualizar a stable_id v2, conviene un `xavier code scan .` (o sync full
  con grafo vacío) para regenerar ids; deltas mezclan ids viejos/nuevos hasta
  reparsear cada archivo tocado.
- Renombrar archivo cambia el path → cambia `stable_id` (esperado).
- Callers fuera del delta sin edge previa pueden quedar stale hasta el próximo
  sync que los toque o un scan completo.
- `file_metadata` sigue usando mtime; `apply_paths` ignora mtime porque la
  lista de paths viene del caller (git).
- Colby sidecar no participa en sync (native CodeGraph only).
- Si `xavier http` tiene abierta `data/code_graph.db`, un `code sync --git`
  local puede esperar el lock SQLite (busy_timeout ~15s). Preferí
  `POST /code/sync` contra el servidor o sincronizar sin el daemon.
- El dump soft de grafos muy grandes (`total_symbols` ≫ 10k) puede tardar.
- `--memory` requiere servidor HTTP alcanzable + `XAVIER_TOKEN`; falla en soft
  (no aborta el sync del grafo).
