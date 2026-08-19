#!/usr/bin/env bash
# Xavier — minimal Linux/macOS install helper.
# Prefer building from source; this script does not download opaque binaries.
set -euo pipefail

echo "Xavier install (source)"
echo "======================="
echo
echo "1) Install a recent Rust toolchain: https://rustup.rs/"
echo "2) From the repo root, build and install the CLI:"
echo
echo "     cargo install --path . --locked"
echo
echo "   Or run without installing:"
echo
echo "     cargo run --release -- http"
echo
echo "3) Set XAVIER_TOKEN in .env (see .env.example) and start:"
echo
echo "     xavier http                 # REST :8006; MCP HTTP :8100 by default"
echo "     xavier http --mcp-port 0    # REST only"
echo "     xavier mcp                  # MCP stdio (Cursor/Claude)"
echo
echo "Docs: README.md, docs/guides/MCP_INTEGRATION.md, docs/guides/CLI_REFERENCE.md"
echo "Agent memory skill: .agents/skills/xavier-memory-protocol/SKILL.md"
echo

if command -v cargo >/dev/null 2>&1; then
  echo "cargo detected: $(cargo --version)"
  echo "Optional: cargo install --path \"$(cd "$(dirname "$0")" && pwd)\" --locked"
else
  echo "cargo not found — install Rust via rustup first."
  exit 1
fi
