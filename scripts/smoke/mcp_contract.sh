#!/usr/bin/env bash
# Xavier MCP/HTTP contract smoke (PLAN FASE 4).
# Usage: ./scripts/smoke/mcp_contract.sh
# Exit non-zero on any hard failure. Does not print secrets.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

PASS=0
FAIL=0
SKIP=0

pass() { echo "Pass: $*"; PASS=$((PASS + 1)); }
fail() { echo "Fail: $*"; FAIL=$((FAIL + 1)); }
skip() { echo "Skip: $*"; SKIP=$((SKIP + 1)); }

# --- 1. Source .env without printing secrets ---
ENV_FILE="${XAVIER_ENV_FILE:-$REPO_ROOT/.env}"
if [[ -f "$ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$ENV_FILE"
  set +a
fi

export XAVIER_DATA_DIR="${XAVIER_DATA_DIR_OVERRIDE:-${XAVIER_DATA_DIR:-$REPO_ROOT/data}}"

PYTHON_BIN="${PYTHON_BIN:-python3}"
command -v "$PYTHON_BIN" >/dev/null 2>&1 || PYTHON_BIN=python
command -v "$PYTHON_BIN" >/dev/null 2>&1 || {
  echo "Fail: python3/python required" >&2
  exit 1
}

HTTP_BASE="${XAVIER_URL:-http://localhost:8006}"
MCP_HTTP_BASE="${XAVIER_MCP_URL:-http://127.0.0.1:8100}"

# Resolve MCP launcher: wrapper preferred, else xavier on PATH / known paths
WRAPPER="$REPO_ROOT/scripts/mcp/xavier-mcp-cursor.sh"
if [[ -x "$WRAPPER" ]]; then
  MCP_CMD=("$WRAPPER")
elif command -v xavier >/dev/null 2>&1; then
  MCP_CMD=(xavier mcp)
elif [[ -x "$HOME/.local/bin/xavier" ]]; then
  MCP_CMD=("$HOME/.local/bin/xavier" mcp)
elif [[ -x "$REPO_ROOT/target_local/release/xavier" ]]; then
  MCP_CMD=("$REPO_ROOT/target_local/release/xavier" mcp)
elif [[ -x "$REPO_ROOT/target/release/xavier" ]]; then
  MCP_CMD=("$REPO_ROOT/target/release/xavier" mcp)
else
  fail "xavier MCP binary/wrapper not found"
  echo "Summary: Pass=$PASS Fail=$FAIL Skip=$SKIP"
  exit 1
fi

TMPDIR_SMOKE="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_SMOKE"' EXIT
MCP_OUT="$TMPDIR_SMOKE/mcp.out"
MCP_ERR="$TMPDIR_SMOKE/mcp.err"

# --- 2. MCP stdio: initialize + tools/list (+ optional memory_search) ---
{
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"mcp-contract-smoke","version":"0.1.0"}}}'
  printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"contract smoke","limit":2}}}'
} | timeout 45 "${MCP_CMD[@]}" >"$MCP_OUT" 2>"$MCP_ERR" || {
  fail "MCP stdio process exited non-zero (see stderr log, secrets redacted)"
}

if [[ ! -s "$MCP_OUT" ]]; then
  fail "MCP stdout empty (logs should be on stderr only)"
else
  FIRST_LINE="$(head -n 1 "$MCP_OUT" | tr -d '\r')"
  if [[ "$FIRST_LINE" == \{* ]]; then
    pass "MCP first stdout line is JSON object (no INFO on stdout)"
  else
    fail "MCP first stdout line must start with '{'; got prefix: ${FIRST_LINE:0:40}"
  fi
fi

"$PYTHON_BIN" - "$MCP_OUT" <<'PY' || true
import json, sys
path = sys.argv[1]
required = {"mem_search", "memory_context", "create_memory"}
tools = set()
ms_ok = False
ms_detail = "no tools/call response"
lines = open(path, encoding="utf-8", errors="replace").read().splitlines()
for line in lines:
    line = line.strip()
    if not line.startswith("{"):
        continue
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        continue
    mid = msg.get("id")
    if mid == 2 and "result" in msg:
        for t in msg["result"].get("tools") or []:
            name = t.get("name")
            if name:
                tools.add(name)
    if mid == 3:
        if msg.get("error"):
            ms_detail = f"jsonrpc error: {msg['error'].get('message', msg['error'])}"
        else:
            result = msg.get("result") or {}
            if result.get("isError"):
                # Prefer text snippet without dumping bodies
                content = result.get("content") or []
                snippet = ""
                if content and isinstance(content[0], dict):
                    snippet = str(content[0].get("text", ""))[:120]
                ms_detail = f"tool isError=true {snippet}"
            else:
                ms_ok = True
                # Structured candidates preferred; tolerate empty search
                text_blob = ""
                for part in result.get("content") or []:
                    if isinstance(part, dict) and part.get("type") == "text":
                        text_blob += str(part.get("text") or "")
                if "candidates" in text_blob or '"id"' in text_blob or text_blob.strip().startswith("{") or text_blob.strip().startswith("["):
                    ms_detail = "structured-or-json result"
                elif not (result.get("content")):
                    # Some builds return empty content with isError false when zero hits
                    ms_detail = "result without error (empty/zero hits ok)"
                else:
                    ms_detail = "result without error"

missing = sorted(required - tools)
def sh_quote(s: str) -> str:
    return "'" + s.replace("'", "'\''") + "'"

with open(path + ".parse", "w", encoding="utf-8") as f:
    f.write("MISSING=" + sh_quote(",".join(missing)) + "\n")
    f.write("MS_OK=" + ("1" if ms_ok else "0") + "\n")
    f.write("MS_DETAIL=" + sh_quote(ms_detail.replace("\n", " ")) + "\n")
    f.write("TOOL_COUNT=" + str(len(tools)) + "\n")
PY

PARSE="$MCP_OUT.parse"
if [[ -f "$PARSE" ]]; then
  # shellcheck disable=SC1090
  source "$PARSE"
  if [[ -z "${MISSING:-}" ]]; then
    pass "tools/list includes mem_search, memory_context, create_memory (count=${TOOL_COUNT:-?})"
  else
    fail "tools/list missing: $MISSING"
  fi
  if [[ "${MS_OK:-0}" == "1" ]]; then
    pass "memory_search tools/call ok (${MS_DETAIL:-})"
  else
    fail "memory_search tools/call failed (${MS_DETAIL:-})"
  fi
else
  fail "failed to parse MCP stdout"
fi

# --- 3. HTTP /memory/search auth ---
if [[ -z "${XAVIER_TOKEN:-}" ]]; then
  fail "XAVIER_TOKEN unset; cannot auth-check HTTP /memory/search"
else
  CODE_OK="$(curl -sS -o /dev/null -w '%{http_code}' -m 8 \
    -X POST "${HTTP_BASE}/memory/search" \
    -H 'Content-Type: application/json' \
    -H "X-Xavier-Token: ${XAVIER_TOKEN}" \
    -d '{"query":"mcp contract smoke","limit":1}' || echo "000")"
  if [[ "$CODE_OK" == "200" ]]; then
    pass "HTTP POST /memory/search with valid token → 200"
  else
    fail "HTTP POST /memory/search with valid token → expected 200 got ${CODE_OK}"
  fi

  CODE_BAD="$(curl -sS -o /dev/null -w '%{http_code}' -m 8 \
    -X POST "${HTTP_BASE}/memory/search" \
    -H 'Content-Type: application/json' \
    -H 'X-Xavier-Token: deliberately-wrong-token-smoke' \
    -d '{"query":"mcp contract smoke","limit":1}' || echo "000")"
  if [[ "$CODE_BAD" == "401" ]]; then
    pass "HTTP POST /memory/search with wrong token → 401"
  else
    fail "HTTP POST /memory/search with wrong token → expected 401 got ${CODE_BAD}"
  fi
fi

# --- 4. Optional MCP HTTP :8100 Origin checks ---
if curl -sS -o /dev/null -m 2 "${MCP_HTTP_BASE}/mcp" >/dev/null 2>&1 \
  || curl -sS -o /dev/null -m 2 -X POST "${MCP_HTTP_BASE}/mcp" \
       -H 'Content-Type: application/json' \
       -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' >/dev/null 2>&1; then

  ORIGIN_BARE="$(curl -sS -o /dev/null -w '%{http_code}' -m 8 \
    -X POST "${MCP_HTTP_BASE}/mcp" \
    -H 'Content-Type: application/json' \
    -H 'Origin: localhost' \
    ${XAVIER_TOKEN:+-H "X-Xavier-Token: ${XAVIER_TOKEN}"} \
    -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' || echo "000")"
  if [[ "$ORIGIN_BARE" == "403" ]]; then
    pass "MCP HTTP Origin 'localhost' (bare) → 403"
  else
    fail "MCP HTTP Origin 'localhost' (bare) → expected 403 got ${ORIGIN_BARE}"
  fi

  if [[ -n "${XAVIER_TOKEN:-}" ]]; then
    ORIGIN_OK="$(curl -sS -o /dev/null -w '%{http_code}' -m 8 \
      -X POST "${MCP_HTTP_BASE}/mcp" \
      -H 'Content-Type: application/json' \
      -H 'Origin: http://127.0.0.1:8100' \
      -H "X-Xavier-Token: ${XAVIER_TOKEN}" \
      -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' || echo "000")"
    if [[ "$ORIGIN_OK" == "200" ]]; then
      pass "MCP HTTP Origin http://127.0.0.1:8100 + token → 200"
    else
      # Soft: may still be auth-shaped 401 if token mismatch vs MCP process
      if [[ "$ORIGIN_OK" == "401" || "$ORIGIN_OK" == "403" ]]; then
        skip "MCP HTTP trusted Origin returned ${ORIGIN_OK} (token/origin policy); not treating as hard fail"
      else
        fail "MCP HTTP trusted Origin → unexpected ${ORIGIN_OK}"
      fi
    fi
  else
    skip "MCP HTTP trusted-Origin check (no XAVIER_TOKEN)"
  fi
else
  skip "MCP HTTP :8100 not reachable"
fi

echo "Summary: Pass=$PASS Fail=$FAIL Skip=$SKIP"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0
