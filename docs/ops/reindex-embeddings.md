# Xavier Embeddings Persistent Reindexing Procedure [OPS-F8]

## Overview
New memories recorded daily in Xavier may initially lack vector embeddings if saved during offline or decoupled indexing modes. To ensure semantic recall remains accurate without overloading the Ollama embedding service (e.g., avoiding high concurrency network errors caused by parallel batching), Xavier relies on a robust persistent reindexing script combined with a single-batch execution policy.

## Reindexing Script (`scripts/reindex-embeddings.sh`)

The reindexing script communicates with the Xavier server endpoint:
`POST /v1/maintenance/reindex-embeddings`

### Features & Protections
1. **Single Instance Guarantee (Lockfile)**: Uses `flock -n /tmp/xavier-reindex.lock` to prevent multiple concurrent reindex operations. If a batch is already in progress, additional invocations are immediately rejected.
2. **Controlled Batch Limit**: Limits background reindexing to a configurable batch size (default: `--limit 500`) to keep resource consumption predictable.
3. **Dry-Run Mode**: Allows operators to query the number of memories lacking embeddings without triggering background processing (`--dry-run`).
4. **Dynamic Token Resolution**: Automatically retrieves `XAVIER_TOKEN` from environment variables, local `.env` files, or running process environments.

### Usage Examples

#### Dry-Run Check
```bash
bash scripts/reindex-embeddings.sh --dry-run
```
Output:
```
[INFO] Starting Xavier reindex embeddings task...
[INFO] Target Endpoint: http://localhost:8006/v1/maintenance/reindex-embeddings
[INFO] Mode: Dry-Run
[INFO] Server Status: ok
[INFO] Memories lacking embeddings (null count): 23092
[INFO] Dry run completed. No background reindexing triggered.
```

#### Trigger Single Batch (Limit: 500)
```bash
bash scripts/reindex-embeddings.sh --limit 500
```

#### Custom Server URL
```bash
bash scripts/reindex-embeddings.sh --limit 500 --url "http://localhost:8006"
```

## Systemd Daily Timer Setup

To schedule daily reindexing using systemd user timers:

### 1. Install Unit Files
Copy or symlink the systemd service and timer unit files into `~/.config/systemd/user/`:

```bash
mkdir -p ~/.config/systemd/user
cp scripts/systemd/xavier-reindex.service ~/.config/systemd/user/
cp scripts/systemd/xavier-reindex.timer ~/.config/systemd/user/
```

### 2. Enable & Start Timer
```bash
systemctl --user daemon-reload
systemctl --user enable --now xavier-reindex.timer
```

### 3. Verify Active Timer
```bash
systemctl --user list-timers | grep -i xavier-reindex
```

### 4. Check Logs
```bash
journalctl --user -u xavier-reindex.service -f
```

## Troubleshooting & Recovery

* **Lockfile Stale Execution Failure**:
  The script uses `flock` bound to file descriptor 200. When the script process exits or crashes, the Linux kernel automatically releases the flock lock on `/tmp/xavier-reindex.lock`.
* **Token Resolution Issues**:
  Ensure `XAVIER_TOKEN` is defined in `.env` at the repository root or in `~/.env`.
