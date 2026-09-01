# CodeGraph git sync

Xavier can incrementally update the code graph from
`git diff` without re-scanning the entire tree file by file.

## Flow

```
git diff --name-status → affected paths → AST reparse → patch symbols/edges → dump JSON
         └─ (optional --memory) → upsert cards in /memory/add  path=code/{repo}/{stable_id}
```

It does not use per-character embeddings: only AST + edges (same as `xavier code scan`).
`stable_id` are **structural (v2)**: `project|file|name|kind|parent|signature`
(without `start_line`), so an intra-file move does not break edges or memory.

## Usage

```bash
# First time / empty graph: does a full scan of `.` and saves checkpoint HEAD
xavier code sync --git

# Incremental vs last checkpoint (.xavier/codegraph-sync-commit)
xavier code sync --git

# Explicit base
xavier code sync --git --base HEAD~3

# Staged only
xavier code sync --git --staged

# Also publish symbol summaries to Xavier memory (cap 80)
xavier code sync --git --memory
```

Also via HTTP (server running): `POST /code/sync` with
`{"git":true,"base":null,"staged":false,"memory":false}`.

## Checkpoint

File: `.xavier/codegraph-sync-commit`  
Contains the SHA of `HEAD` after a successful sync. If it does not exist and the graph already
has symbols, the default is `HEAD~1`.

## Optional hook (post-commit)

Not installed automatically. Idempotent installation:

```bash
bash scripts/hooks/install-post-commit-codegraph.sh
```

Or manually:

```bash
ln -sf ../../scripts/hooks/post-commit-codegraph.sh .git/hooks/post-commit
```

The hook soft-fails: a sync error does not block the commit.

## Doctor

`xavier doctor` includes a soft **CodeGraph Index** check:
`total_symbols == 0` → Warn (exit code remains 0 if the rest is OK).

`GET /code/stats` also marks `"degraded": true` when the graph is empty.

## Known limitations

- After upgrading to stable_id v2, a `xavier code scan .` (or full sync
  with empty graph) is recommended to regenerate ids; deltas mix old/new ids until
  each touched file is reparsed.
- Renaming a file changes the path → changes `stable_id` (expected).
- Callers outside the delta without prior edge may remain stale until the next
  sync that touches them or a full scan.
- `file_metadata` still uses mtime; `apply_paths` ignores mtime because the
  path list comes from the caller (git).
- Colby sidecar does not participate in sync (native CodeGraph only).
- If `xavier http` has `data/code_graph.db` open, a local `code sync --git`
  may wait for the SQLite lock (busy_timeout ~15s). Prefer
  `POST /code/sync` against the server or sync without the daemon.
- Soft dump of very large graphs (`total_symbols` ≫ 10k) may take time.
- `--memory` requires reachable HTTP server + `XAVIER_TOKEN`; soft-fails
  (does not abort the graph sync).
- Sync **skips** the soft-dump when `total_symbols > 25000` threshold
  (`DUMP_SOFT_SKIP_SYMBOLS`); use `xavier code dump .` explicitly.
- `POST /code/sync` uses the server's `workspace_dir` (not the process `cwd`).
