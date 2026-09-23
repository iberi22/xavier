# Xavier — Windows Installation

## Requirements

- Windows 10/11
- Optional: Rust toolchain (only needed if building from source) → https://rustup.rs
- Optional: Ollama (for local embeddings) → https://ollama.com

## Quick Install (No Rust / C++ Compiler Required)

You can install Xavier with a single PowerShell command without installing Rust or compiling from source. The installer automatically downloads prebuilt binaries and Web UI assets from GitHub Releases:

```powershell
irm https://raw.githubusercontent.com/iberi22/xavier/main/scripts/install.ps1 | iex
```

### Script Options

When running `install.ps1` locally from a cloned repository or downloaded script:

```powershell
# Automatically download latest prebuilt release binary and UI assets
.\scripts\install.ps1 -DownloadLatest

# Download a specific release version
.\scripts\install.ps1 -Version "0.2.0"
```

## Building & Installing from Source

If you prefer building from source:

```powershell
# 1. Build binary (from project root)
cargo build --release --bin xavier -j 1

# 2. Build Web UI (optional, for web dashboard)
cd panel-ui
pnpm install
pnpm build
cd ..

# 3. Run installer
.\scripts\install.ps1
```

## What the Installer Does

This installer:
- Deploys `xavier.exe` to `%LOCALAPPDATA%\Xavier\bin\`
- Copies Web UI assets (`panel-ui/build`) to `%LOCALAPPDATA%\Xavier\panel-ui\build`
- Adds `%LOCALAPPDATA%\Xavier\bin\` to the User `PATH`
- Generates a random secure `XAVIER_TOKEN`
- Creates `%LOCALAPPDATA%\Xavier\data\` for local SQLite database storage
- Copies `.env.example` to `%LOCALAPPDATA%\Xavier\config\.env`

> **Restart your terminal** after installation for the `PATH` environment variable changes to take effect.

## Usage

```powershell
# Help
xavier --help

# Start server (web dashboard available at http://localhost:8006)
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
