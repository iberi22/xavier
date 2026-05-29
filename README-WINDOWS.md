# Xavier v0.6.1-beta - Windows Installation Guide

> ⚡ **One-liner install** (PowerShell as Administrator):
> ```powershell
> irm https://raw.githubusercontent.com/iberi22/xavier/main/install.ps1 | iex
> ```

---

## What's New in v0.6.1-beta (vs Xavier2)

Xavier v0.6.1-beta is the evolution of Xavier2, now with enterprise-grade features while maintaining full backward compatibility.

### New Features
- **Context Regeneration** — Multi-phase context rebuilding (Phase 0, 1, 2)
- **Multi-Provider Agent Spawn** — Spawn agents with MiniMax, DeepSeek, OpenAI routing
- **WebSocket Streaming** — Real-time event streaming
- **BM25 Hybrid Search** — Combined vector + keyword search
- **Unified MCP** — Model Context Protocol server (stdio)
- **Belief Graph & GraphRAG** — Hierarchical memory with semantic relationships
- **Native Security Scanner** — Multi-layer prompt injection detection
- **Auto-Verification** — Save/retrieve verification loop (save_ok, latency_ms, match_score)
- **Session Sync** — Background cron with lag monitoring
- **Panel UI** — Built-in web dashboard
- **TUI Dashboard** — Terminal UI with ratatui

### Backward Compatible
All Xavier2 endpoints work unchanged:
- `POST /memory/search` — Search with `X-Xavier-Token` or `Authorization: Bearer`
- `POST /memory/add` — Add memory
- `POST /xavier/verify/save` — Verify save/retrieve cycle
- `GET  /health` — Health check

---

## Prerequisites

| Requirement | Version | Notes |
|-------------|---------|-------|
| Windows | 10/11 | 64-bit |
| PowerShell | 5.1+ | Or PowerShell 7+ |
| Rust | 1.80+ | Installed automatically if missing |
| Git | 2.40+ | Required |
| CMake | 3.25+ | For libsql-ffi compilation |
| Node.js | 22.12+ | Optional (for Panel UI) |
| Disk space | 10 GB | 2 GB for install, 8 GB for cargo cache |

---

## Installation Options

### Option 1: One-liner (Recommended)

```powershell
irm https://raw.githubusercontent.com/iberi22/xavier/main/install.ps1 | iex
```

This will:
1. Install Rust if not present (via rustup)
2. Clone the repository
3. Build the binary (10-30 min first time)
4. Configure environment variables
5. Create startup scripts
6. Run verification tests

### Option 2: With custom settings

```powershell
irm https://raw.githubusercontent.com/iberi22/xavier/main/install.ps1 | iex `
    -InstallDir "D:\Xavier" `
    -Port 8006 `
    -Token "your-secure-token" `
    -AsService
```

### Option 3: Manual build

```powershell
# 1. Clone
git clone https://github.com/iberi22/xavier.git
cd xavier

# 2. Build
cargo build --release --bin xavier --features "local-gllm,cli-interactive"

# 3. Configure
$env:XAVIER_TOKEN = "your-secure-token"
$env:XAVIER_PORT = "8006"

# 4. Run
.\target\release\xavier.exe http
```

---

## Post-Installation

### Directory Structure
```
%USERPROFILE%\.xavier\
├── xavier.exe              # Main binary
├── config\
│   └── xavier.config.json  # Runtime config
├── data\                   # SQLite databases
│   ├── xavier.db
│   └── code_graph.db
├── logs\                   # Log files
├── start.bat               # Quick start
├── verify.ps1              # Verification script
├── install-service.bat     # Install as Windows Service
└── uninstall.bat           # Cleanup
```

### Start the Server
```powershell
# Interactive
~\.xavier\start.bat

# Or directly
~\.xavier\xavier.exe http --config ~\.xavier\config\xavier.config.json

# With custom port
$env:XAVIER_PORT = 9000
~\.xavier\xavier.exe http
```

### Verify Installation
```powershell
~\.xavier\verify.ps1
```

Expected output:
```
=== Xavier v0.6.1-beta Verification ===

[1/5] Health check...
  Health: ok

[2/5] Auth check...
  Auth OK, workspace: default

[3/5] Memory add...
  Added: mem_xxx

[4/5] Memory search...
  Found: 1 results

[5/5] Sync check...
  Status: ok, Agents: 0
```

### Install as Windows Service (Run as Admin)
```powershell
~\.xavier\install-service.bat
```

Or manually:
```powershell
sc create Xavier binPath= "C:\Users\<you>\.xavier\xavier.exe http --config C:\Users\<you>\.xavier\config\xavier.config.json" start= auto
sc description Xavier "Xavier v0.6.1-beta - AI Agent Memory Runtime"
sc start Xavier
```

---

## API Quick Reference

### Authentication
Both headers are accepted:
```bash
# Xavier2 style
curl -H "X-Xavier-Token: your-token" ...

# OAuth2 style
curl -H "Authorization: Bearer your-token" ...
```

### Add Memory
```powershell
$body = @{
    content = "Agent guidelines for SouthWest AI Labs"
    path = "swal/guidelines"
    metadata = @{ project = "xavier"; priority = "high" }
} | ConvertTo-Json -Depth 3

Invoke-RestMethod -Uri "http://localhost:8006/memory/add" `
    -Method POST -Headers @{ "X-Xavier-Token" = "your-token"; "Content-Type" = "application/json" } `
    -Body $body
```

### Search Memory
```powershell
$body = @{ query = "agent guidelines"; limit = 10 } | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:8006/memory/search" `
    -Method POST -Headers @{ "X-Xavier-Token" = "your-token"; "Content-Type" = "application/json" } `
    -Body $body
```

### Verify Save/Retrieve
```powershell
$body = @{
    path = "swal/guidelines"
    content = "Agent guidelines for SouthWest AI Labs"
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:8006/xavier/verify/save" `
    -Method POST -Headers @{ "X-Xavier-Token" = "your-token"; "Content-Type" = "application/json" } `
    -Body $body
```

---

## Troubleshooting

### Build fails with "program not found"
**Cause**: CMake or C compiler missing  
**Fix**: Install Visual Studio Build Tools or MinGW:
```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --add Microsoft.VisualStudio.Workload.VCTools"
# Or
winget install MinGW.MinGW
```

### "libsql-ffi build failed"
**Cause**: Missing clang/gcc on Windows  
**Fix**: The installer auto-installs prerequisites. If manual:
```powershell
winget install LLVM.LLVM  # For clang
```

### Out of disk space during build
**Cause**: Cargo target cache grows large  
**Fix**: Clean cache or move to larger drive:
```powershell
cargo clean
# Or set CARGO_TARGET_DIR to another drive
$env:CARGO_TARGET_DIR = "D:\cargo-target"
```

### Port 8006 already in use
```powershell
# Find process
Get-NetTCPConnection -LocalPort 8006 | Select-Object OwningProcess
# Kill or change port in config
```

### Service won't start
Check logs:
```powershell
Get-EventLog -LogName Application -Source "Xavier" -Newest 10
# Or run interactively to see errors
~\.xavier\xavier.exe http
```

---

## Upgrading from Xavier2

Xavier2 data is fully compatible. The migration path:

1. Stop Xavier2 (if running)
2. Run the v0.6.1-beta installer
3. Point config to old data directory:
   ```json
   {
     "memory": {
       "store_path": "C:/old/path/to/xavier2.db"
     }
   }
   ```
4. Start v0.6.1-beta — all memories preserved

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `XAVIER_TOKEN` | — | Auth token (required) |
| `XAVIER_PORT` | 8006 | HTTP port |
| `XAVIER_HOST` | 127.0.0.1 | Bind address |
| `XAVIER_HOME` | ~/.xavier | Installation directory |
| `XAVIER_CONFIG_PATH` | — | Config file path |
| `RUST_LOG` | info | Log level |
| `XAVIER_WORKSPACE_ID` | default | Default workspace |

---

## Support

- **Issues**: https://github.com/iberi22/xavier/issues
- **Docs**: https://github.com/iberi22/xavier/tree/main/docs
- **Enterprise**: enterprise@southwest-ai-labs.com

---

*Xavier v0.6.1-beta — Built with Rust, powered by SWAL*
