#!/usr/bin/env python3
"""
generate_mcp_configs.py — Genera los .mcp.json / settings por CLI para que cada
coding-agent CLI tenga a Xavier disponible como servidor MCP.

Lee XAVIER_TOKEN del .env (no hardcodea nada). Los archivos generados con el token
real se escriben en scripts/subagents/mcp/GENERATED/ (que esta en .gitignore).

CLIs soportados:
  - opencode (Go)  -> opencode.mcp.json (formato estandar mcpServers)
  - gemini         -> gemini.settings.json (formato .gemini/settings.json)
  - claude (Code)  -> claude.mcp.json (formato --mcp-config)
  - codex          -> codex.system-prompt.md (codex exec no soporta MCP; usa curl fallback)

Uso:
  python generate_mcp_configs.py            # genera todos
  python generate_mcp_configs.py --cli opencode   # solo uno
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ENV_FILE = REPO_ROOT / ".env"
OUT_DIR = Path(__file__).resolve().parent / "mcp" / "GENERATED"
OUT_DIR.mkdir(parents=True, exist_ok=True)

XAVIER_EXE = r"C:\Users\belal\bin\xavier.exe"
DATA_DIR = str(REPO_ROOT / "data").replace("\\", r"\\")
TOKEN_SECRET = "swal-secret-2026"


def read_token() -> str:
    if not ENV_FILE.exists():
        print(f"ERROR: no existe {ENV_FILE}", file=sys.stderr)
        sys.exit(1)
    for line in ENV_FILE.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line.startswith("XAVIER_TOKEN=") and not line.startswith("#"):
            return line.split("=", 1)[1].strip()
    print("ERROR: XAVIER_TOKEN no encontrado en .env", file=sys.stderr)
    sys.exit(1)


def env_block(token: str) -> dict:
    return {
        "XAVIER_TOKEN": token,
        "XAVIER_TOKEN_SECRET": TOKEN_SECRET,
        "XAVIER_DATA_DIR": DATA_DIR,
        "XAVIER_EMBEDDING_PROVIDER_MODE": os.environ.get("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud"),
    }


def gen_opencode(token: str) -> Path:
    """opencode lee opencode.json con clave 'mcp' (NO 'mcpServers').
    Usa type 'remote' apuntando al MCP HTTP del server (puerto 8100) — mas rapido que
    stdio local (evita lanzar un proceso xavier.exe pesado por llamada).
    """
    mcp_port = os.environ.get("XAVIER_MCP_PORT", "8100")
    cfg = {
        "$schema": "https://opencode.ai/config.json",
        "mcp": {
            "xavier-memory": {
                "type": "remote",
                "url": f"http://localhost:{mcp_port}/mcp",
                "headers": {
                    "X-Xavier-Token": token,
                    "Origin": "localhost",
                },
                "enabled": True,
            }
        },
    }
    out = OUT_DIR / "opencode.mcp.json"
    out.write_text(json.dumps(cfg, indent=2), encoding="utf-8")
    # tambien generamos la variante stdio (local) como fallback
    local_cfg = {
        "$schema": "https://opencode.ai/config.json",
        "mcp": {
            "xavier-memory": {
                "type": "local",
                "command": [XAVIER_EXE, "mcp"],
                "env": env_block(token),
                "enabled": True,
            }
        },
    }
    (OUT_DIR / "opencode.local.mcp.json").write_text(json.dumps(local_cfg, indent=2), encoding="utf-8")
    return out


def gen_gemini(token: str) -> Path:
    """gemini CLI lee .gemini/settings.json con clave mcpServers."""
    cfg = {
        "mcpServers": {
            "xavier-memory": {
                "command": XAVIER_EXE,
                "args": ["mcp"],
                "env": env_block(token),
            }
        }
    }
    out = OUT_DIR / "gemini.settings.json"
    out.write_text(json.dumps(cfg, indent=2), encoding="utf-8")
    return out


def gen_claude(token: str) -> Path:
    """claude (Claude Code) usa --mcp-config <file>; formato mcpServers."""
    cfg = {
        "mcpServers": {
            "xavier-memory": {
                "command": XAVIER_EXE,
                "args": ["mcp"],
                "env": env_block(token),
            }
        }
    }
    out = OUT_DIR / "claude.mcp.json"
    out.write_text(json.dumps(cfg, indent=2), encoding="utf-8")
    return out


def gen_codex(token: str) -> Path:
    """codex exec NO soporta MCP. Fallback: system-prompt con instrucciones curl."""
    prompt = f"""# Xavier Brain — Codex fallback (curl, no MCP)

Codex CLI no soporta servidores MCP nativos, asi que interactuas con Xavier via HTTP curl.

## Configuracion
- Xavier URL: http://localhost:8006
- Token (header X-Xavier-Token): {token}

## Protocolo

### 1. Recall (ANTES de trabajar)
```bash
curl -s -X POST -H "X-Xavier-Token: {token}" -H "Content-Type: application/json" \\
  http://localhost:8006/memory/search \\
  -d '{{"query":"<tu pregunta>","limit":5,"filters":{{"path_prefix":"<tu-proyecto>/"}}}}'
```
Lee los resultados y usalos.

### 2. Persist (DESPUES)
```bash
curl -s -X POST -H "X-Xavier-Token: {token}" -H "Content-Type: application/json" \\
  http://localhost:8006/memory/add \\
  -d '{{"path":"<tipo>/<slug>","content":"<hecho autosuficiente>","metadata":{{"kind":"decision"}}}}'
```

IMPORTANTE: si no puedes ejecutar curl (sandbox), informa al orquestador que necesitas
modo `workspace-write` o que use opencode/claude que SI soportan MCP nativo.
"""
    out = OUT_DIR / "codex.system-prompt.md"
    out.write_text(prompt, encoding="utf-8")
    return out


GENERATORS = {
    "opencode": gen_opencode,
    "gemini": gen_gemini,
    "claude": gen_claude,
    "codex": gen_codex,
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli", choices=list(GENERATORS) + ["all"], default="all")
    args = ap.parse_args()
    token = read_token()
    clis = list(GENERATORS) if args.cli == "all" else [args.cli]
    for cli in clis:
        out = GENERATORS[cli](token)
        print(f"[ok] {cli:10s} -> {out}")
    # recordatorio de gitignore
    gitignore = REPO_ROOT / ".gitignore"
    entries = gitignore.read_text(encoding="utf-8").splitlines() if gitignore.exists() else []
    marker = "scripts/subagents/mcp/GENERATED/"
    if marker not in entries:
        with open(gitignore, "a", encoding="utf-8") as f:
            if entries and entries[-1].strip():
                f.write("\n")
            f.write(f"# Token-bearing MCP configs (auto-generated)\n{marker}\n")
        print(f"[ok] anadido {marker} a .gitignore")


if __name__ == "__main__":
    main()
