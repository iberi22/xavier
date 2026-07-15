# Xavier Deployment Guide

Version: `0.10.0-12-06-2026`

Default service URL: `http://localhost:8006`

## Required Runtime Settings

| Variable | Required | Description |
|---|---|---|
| `XAVIER_TOKEN` | yes | API token required by protected endpoints through `X-Xavier-Token`. |
| `XAVIER_HOST` | no | Bind host. Use `0.0.0.0` in containers. |
| `XAVIER_PORT` | no | Bind port, usually `8006`. |
| `XAVIER_WORKSPACE_DIR` | recommended | Workspace and runtime state directory. |
| `XAVIER_MEMORY_BACKEND` | no | Memory backend, commonly `vec`. |
| `XAVIER_MEMORY_SQLITE_PATH` | recommended | SQLite memory store path. |
| `XAVIER_MEMORY_VEC_PATH` | recommended | SQLite-Vec store path. |
| `XAVIER_CODE_GRAPH_DB_PATH` | recommended | Code graph database path. |
| `XAVIER_EMBEDDING_URL` | optional | Embedding provider endpoint. |
| `XAVIER_EMBEDDING_MODEL` | optional | Embedding model. |
| `XAVIER_MODEL_PROVIDER` | optional | LLM provider selector. |
| `RUST_LOG` | optional | Rust tracing log level. |

Generate a token:

```bash
xavier token new
```

## Docker

Use the GHCR image:

```bash
docker pull ghcr.io/iberi22/xavier:latest

docker run -d --name xavier \
  --restart unless-stopped \
  -p 8006:8006 \
  -e XAVIER_TOKEN="$XAVIER_TOKEN" \
  -e XAVIER_HOST=0.0.0.0 \
  -e XAVIER_PORT=8006 \
  -e XAVIER_WORKSPACE_DIR=/data/workspaces \
  -e XAVIER_MEMORY_BACKEND=vec \
  -e XAVIER_MEMORY_SQLITE_PATH=/data/memory-store.sqlite3 \
  -e XAVIER_MEMORY_VEC_PATH=/data/vec-store.sqlite3 \
  -e XAVIER_CODE_GRAPH_DB_PATH=/data/code_graph.db \
  -v xavier_data:/data \
  ghcr.io/iberi22/xavier:latest
```

Health check:

```bash
curl http://localhost:8006/health
```

Authenticated check:

```bash
curl http://localhost:8006/memory/stats \
  -H "X-Xavier-Token: $XAVIER_TOKEN"
```

## Docker Compose

Minimal `docker-compose.yml`:

```yaml
services:
  xavier:
    image: ghcr.io/iberi22/xavier:latest
    container_name: xavier
    restart: unless-stopped
    ports:
      - "8006:8006"
    environment:
      XAVIER_TOKEN: ${XAVIER_TOKEN:?Set XAVIER_TOKEN}
      XAVIER_HOST: 0.0.0.0
      XAVIER_PORT: 8006
      XAVIER_MEMORY_BACKEND: vec
      XAVIER_WORKSPACE_DIR: /data/workspaces
      XAVIER_MEMORY_SQLITE_PATH: /data/memory-store.sqlite3
      XAVIER_MEMORY_VEC_PATH: /data/vec-store.sqlite3
      XAVIER_CODE_GRAPH_DB_PATH: /data/code_graph.db
      RUST_LOG: ${RUST_LOG:-info}
    volumes:
      - xavier_data:/data
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8006/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 15s

volumes:
  xavier_data:
```

Run:

```bash
export XAVIER_TOKEN="$(xavier token new | tail -n 1)"
docker compose up -d
docker compose logs -f xavier
```

The repository also includes Docker Compose variants under `docker/` for local development, embeddings, benchmarks, and enterprise/plugin setups.

## Persistent Storage

Persist these paths across restarts:

| Path | Purpose |
|---|---|
| `/data/workspaces` | Workspace state. |
| `/data/memory-store.sqlite3` | Primary SQLite memory state. |
| `/data/vec-store.sqlite3` | Vector-backed memory state. |
| `/data/code_graph.db` | Code graph index. |
| `/data/sync` | Mesh sync chunks/manifests when generated under the runtime data directory. |

Recommended backup command for Docker volumes:

```bash
docker run --rm \
  -v xavier_data:/data:ro \
  -v "$PWD/backups:/backup" \
  alpine tar -czf /backup/xavier-data-$(date +%Y%m%d-%H%M%S).tar.gz -C /data .
```

On Windows PowerShell:

```powershell
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
docker run --rm -v xavier_data:/data:ro -v "${PWD}\backups:/backup" alpine `
  tar -czf "/backup/xavier-data-$stamp.tar.gz" -C /data .
```

## Linux systemd Service

Install the binary:

```bash
cargo install --path .
sudo install -m 0755 ~/.cargo/bin/xavier /usr/local/bin/xavier
```

Create a service user and directories:

```bash
sudo useradd --system --home /var/lib/xavier --shell /usr/sbin/nologin xavier
sudo mkdir -p /var/lib/xavier/workspaces /etc/xavier
sudo chown -R xavier:xavier /var/lib/xavier
```

Create `/etc/xavier/xavier.env`:

```bash
XAVIER_TOKEN=change-me
XAVIER_HOST=0.0.0.0
XAVIER_PORT=8006
XAVIER_WORKSPACE_DIR=/var/lib/xavier/workspaces
XAVIER_MEMORY_BACKEND=vec
XAVIER_MEMORY_SQLITE_PATH=/var/lib/xavier/memory-store.sqlite3
XAVIER_MEMORY_VEC_PATH=/var/lib/xavier/vec-store.sqlite3
XAVIER_CODE_GRAPH_DB_PATH=/var/lib/xavier/code_graph.db
RUST_LOG=info
```

Create `/etc/systemd/system/xavier.service`:

```ini
[Unit]
Description=Xavier Memory Runtime
After=network-online.target
Wants=network-online.target

[Service]
User=xavier
Group=xavier
EnvironmentFile=/etc/xavier/xavier.env
ExecStart=/usr/local/bin/xavier http 8006
Restart=on-failure
RestartSec=5
WorkingDirectory=/var/lib/xavier
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

Enable:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now xavier
sudo systemctl status xavier
curl http://localhost:8006/health
```

## Windows Scheduled Task

Create directories:

```powershell
New-Item -ItemType Directory -Force C:\xavier\data | Out-Null
New-Item -ItemType Directory -Force C:\xavier\logs | Out-Null
```

Set machine-level environment variables:

```powershell
[Environment]::SetEnvironmentVariable("XAVIER_TOKEN", "change-me", "Machine")
[Environment]::SetEnvironmentVariable("XAVIER_HOST", "127.0.0.1", "Machine")
[Environment]::SetEnvironmentVariable("XAVIER_PORT", "8006", "Machine")
[Environment]::SetEnvironmentVariable("XAVIER_WORKSPACE_DIR", "C:\xavier\data\workspaces", "Machine")
[Environment]::SetEnvironmentVariable("XAVIER_CODE_GRAPH_DB_PATH", "C:\xavier\data\code_graph.db", "Machine")
```

Register a scheduled task:

```powershell
$exe = "$env:USERPROFILE\.cargo\bin\xavier.exe"
$action = New-ScheduledTaskAction -Execute $exe -Argument "http 8006" -WorkingDirectory "C:\xavier"
$trigger = New-ScheduledTaskTrigger -AtStartup
$settings = New-ScheduledTaskSettingsSet -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName "Xavier" -Action $action -Trigger $trigger -Settings $settings -RunLevel Highest -Description "Xavier Memory Runtime"
Start-ScheduledTask -TaskName "Xavier"
```

Check:

```powershell
Invoke-WebRequest -UseBasicParsing http://localhost:8006/health
```

To stop:

```powershell
Stop-ScheduledTask -TaskName "Xavier"
```

## Health Checks

Unauthenticated:

```bash
curl -fsS http://localhost:8006/health
curl -fsS http://localhost:8006/readiness
curl -fsS http://localhost:8006/build
```

Authenticated:

```bash
curl -fsS http://localhost:8006/memory/stats \
  -H "X-Xavier-Token: $XAVIER_TOKEN"

curl -fsS http://localhost:8006/v1/mesh/identity \
  -H "X-Xavier-Token: $XAVIER_TOKEN"
```

Docker health checks should target `/health`. External load balancers can use `/readiness` when they need a readiness-specific probe.

## CI/CD Pipeline Integration

The repository includes GitHub Actions workflows for:

| Workflow | Purpose |
|---|---|
| `.github/workflows/ci.yml` | Multi-OS Rust formatting, check, clippy, tests, docs tests, coverage, release build, panel validation, Playwright E2E, and release smoke. |
| `.github/workflows/docker-build.yml` | Docker Buildx publishing to GHCR for `linux/amd64` and `linux/arm64`. |
| `.github/workflows/release.yml` | Release assets for Linux, Windows, and macOS. |
| `.github/workflows/build-windows.yml` | Windows binary and ZIP release assets. |
| `.github/workflows/tauri-release.yml` | Desktop app release packaging. |
| `.github/workflows/docs.yml` | Documentation and DevLog site generation/deployment. |
| `.github/workflows/data-commons-test.yml` | Data Commons simulator, crypto gating, anonymizer, DAO governance, remediation, and CVE-learning tests. |
| `.github/workflows/cortex-index.yml` | Containerized index generation and health-gated indexing. |

Recommended release flow:

1. Run local checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features ci-safe --exclude xavier-web --exclude app -- -D warnings
cargo test --workspace --features ci-safe --exclude xavier-web --exclude app
```

2. Build the release binary:

```bash
cargo build --release --bin xavier
```

3. Build and smoke test Docker:

```bash
docker build -t xavier:local -f docker/Dockerfile .
docker run --rm -p 8006:8006 -e XAVIER_TOKEN=test -e XAVIER_HOST=0.0.0.0 xavier:local
```

4. Tag and push. GitHub Actions handles matrix checks, Docker images, and release artifacts.

## Upgrade Procedure

1. Back up persistent storage.
2. Pull the new binary or image.
3. Restart the service.
4. Verify `/health`, `/readiness`, and one authenticated endpoint.
5. Run a small memory add/search smoke test.

Smoke test:

```bash
curl -X POST http://localhost:8006/memory/add \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content":"deployment smoke test","path":"smoke/deployment"}'

curl -X POST http://localhost:8006/memory/search \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query":"deployment smoke test","limit":1}'
```


## 🧩 Environment Detection (Post-Install)

Después de la instalación, verificar el entorno:

### Verificar estado
```bash
# 1. Health check
curl http://localhost:8006/health

# 2. Detectar entorno
bash /home/belal/.hermes/scripts/which-xavier.sh

# 3. Verificar memoria
curl -X POST http://localhost:8006/memory/search \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query":"test", "limit":1}'

# 4. Verificar code-graph
curl http://localhost:8006/code/stats -H "X-Xavier-Token: $XAVIER_TOKEN"

# 5. Verificar bridge
pgrep -f xavier-mcp-bridge
```

### Arranque Limpio
```bash
bash /home/belal/.hermes/scripts/start-xavier.sh
```

### Troubleshooting

| Problema | Síntoma | Fix |
|----------|---------|-----|
| Bridges zombies | Múltiples instancias `pgrep -f xavier-mcp-bridge` | Matar todos y dejar que Hermes los respawnee |
| Token inválido | `401 Unauthorized` | Verificar `/home/belal/.xavier/.env` |
| Code-graph vacío | `0 files` en `/code/stats` | Ejecutar `./code-graph scan --no-incremental ./src` |
