# TOOLS.md — Herramientas Locales / Local Tools

> Notas sobre herramientas disponibles en el entorno de desarrollo de Xavier.
> Notes on tools available in the Xavier development environment.

---

## 🦀 Rust Toolchain

### Comandos Esenciales
```bash
# Build & check
cargo check                  # Fast compilation check
cargo build --release        # Release build
cargo clippy -- -D warnings  # Lint

# Test
cargo test --lib             # Unit tests (fast)
cargo test --tests           # All integration tests
cargo test -- --nocapture    # With output

# Run
cargo run -- http 8006       # Start HTTP server
cargo run -- mcp             # Start MCP stdio server

# Dependencies
cargo update                 # Update lockfile
cargo audit                  # Security audit
```

### Features Flags
```bash
cargo build --features telegram    # Build with Telegram bot
cargo test --features telegram     # Test with Telegram
```

---

## 🐳 Docker

```bash
# Start all services
docker compose up -d

# Start Xavier only
docker compose up -d xavier

# Check logs
docker compose logs -f xavier

# Health check
curl http://localhost:8006/health
```

### Containers Principales
| Container | Puerto | Propósito |
|-----------|--------|-----------|
| xavier | 8006 | HTTP API + MCP |
| pgheart-postgres | 5432 | PostgreSQL (plugin) |
| pplx-embed | 8002 | Embedding provider |

---

## 🧠 Xavier Server

### Inicio
```powershell
# Windows (optimizado RAG)
.\start-xavier-rag.ps1
```

```bash
# Manual (cualquier SO)
xavier http 8006
```

### CLI Commands
```bash
xavier stats                  # Memory stats
xavier add "text" "path"      # Add memory
xavier search "query"         # Search memory
xavier token new              # Generate API token
xavier code scan .            # Scan codebase
xavier mesh id                # Show node identity
xavier license status         # Show license
xavier improve run            # Run auto-improvement
xavier improve status         # Check improvement status
```

### API Endpoints (protegidos con X-Xavier-Token)
| Method | Path | Descripción |
|--------|------|-------------|
| GET | `/health` | Health check |
| POST | `/memory/add` | Add memory |
| POST | `/v1/memories/search` | Hybrid search |
| GET | `/memory/stats` | Statistics |
| POST | `/code/scan` | Code scanning |
| POST | `/mesh/sync` | Mesh sync |

---

## 🐍 Scripts y Herramientas

### PowerShell
```powershell
.\scripts\xavier-helper.ps1 -Action search -Query "..."
.\scripts\xavier-helper.ps1 -Action add -Content "..." -Category decisions
.\scripts\xavier-helper.ps1 -Action health
.\scripts\xavier-helper.ps1 -Action stats
```

### Python
```bash
# Benchmarks
python scripts/benchmarks/run_locomo_benchmark.py

# Audio transcription
python E:\scripts-python\scripts\audio-to-text.py audio.ogg --language es
```

### Subagentes
```bash
# Dispatcher
python scripts/subagents/dispatch.py

# Reports
ls scripts/subagents/reports/
```

---

## 🖥️ SWAL Node (Termux)

```bash
# SSH via Cloudflare tunnel
ssh termux-cf

# Commands
swal-node.sh docker       # Docker status
swal-node.sh xavier       # Xavier status
swal-node.sh tunnel       # Tunnel status
swal-node.sh restart      # Restart services
```

---

## ☁️ Cloudflare Tunnel

```bash
# Check tunnel status
cloudflared tunnel list

# Route to Xavier
cloudflared tunnel route dns <tunnel-id> xavier.swal.dev
```

---

## 📦 Package Managers

| Tool | Comando | Notas |
|------|---------|-------|
| Rust | `cargo` | v1.80+ |
| Node | `nvm use && npm` | Para panel-ui |
| Python | `python` / `pip` | Scripts auxiliares |
| Docker | `docker compose` | Servicios |

---

## 🔐 Credenciales

- **Xavier Token:** `$env:XAVIER_TOKEN` o en `.env`
- **GitHub:** `gh auth status`
- **Docker Hub:** `docker login`
- **Vault Clavis:** Hardware vault para secrets sensibles

---

_Última actualización: 2026-07-09_
