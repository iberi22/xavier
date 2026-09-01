# Xavier — Windows Installation

## Requirements

- Windows 10/11
- Rust toolchain (only to compile) → https://rustup.rs
- Optional: Ollama (for local embeddings) → https://ollama.com

## Quick Install

```powershell
# 1. Build (from project root)
cargo build --release --bin xavier -j 1

# 2. Run installer (as Administrator)
.\install.ps1
```

This:
- Copies `xavier.exe` to `%LOCALAPPDATA%\Xavier\bin\`
- Adds that folder to the user PATH
- Generates a random `XAVIER_TOKEN`
- Creates `%LOCALAPPDATA%\Xavier\data\` for the database
- Copies `.env.example` as base config

> **Restart your terminal** after installing for the PATH to take effect.

## Usage

```powershell
# Help
xavier --help

# Start server
xavier serve

# Live monitor
xavier monitor

# Check status
xavier status
```

## Configuration

Edit `%LOCALAPPDATA%\Xavier\config\.env` with your values:

| Variable | Description | Default |
|---|---|---|
| `XAVIER_TOKEN` | API token (required) | (generated) |
| `XAVIER_PORT` | Server port | 8003 |
| `XAVIER_MEMORY_BACKEND` | Backend: `vec` (SQLite) or `surreal` | `vec` |
| `XAVIER_EMBEDDING_URL` | Embeddings URL (Ollama) | `http://localhost:11434/v1` |
| `XAVIER_EMBEDDING_MODEL` | Embeddings model | `nomic-embed-text` |
| `XAVIER_MODEL_PROVIDER` | LLM provider | `local` |
| `RUST_LOG` | Logging level | `info` |

## Docker (alternative)

```powershell
docker compose --profile core up -d
```

## Uninstall

```powershell
# Remove from PATH
$path = [Environment]::GetEnvironmentVariable("PATH", "User")
$path = ($path.Split(';') | Where-Object { $_ -ne "$env:LOCALAPPDATA\Xavier\bin" }) -join ';'
[Environment]::SetEnvironmentVariable("PATH", $path, "User")

# Delete directory
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Xavier"
```
