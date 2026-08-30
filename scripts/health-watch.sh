#!/usr/bin/env bash
# WAVEX-13-08: Guardia diaria del loop de memoria de Xavier.
# Verifica: (a) servidor vivo, (b) escrituras nuevas en 48h, (c) % de memorias
# con embedding, (d) recall devuelve resultados con σ>0 (búsqueda semántica viva).
# Salida JSON + exit code != 0 si algo falla (apto para cron/alerta).
#
# Uso:
#   bash scripts/health-watch.sh
#   bash scripts/health-watch.sh --json-only
#
# Cron de ejemplo (cada día 08:00):
#   0 8 * * *  xavier health-watch >> /var/log/xavier-health.log 2>&1
#
# Dependencias: curl, sqlite3, jq (opcional; sin jq usa python3).

set -uo pipefail

XAVIER_URL="${XAVIER_URL:-http://localhost:8006}"
TOKEN_FILE="${XAVIER_TOKEN_FILE:-.env}"
VEC_DB="${VEC_DB:-data/vec-store.sqlite3}"
WINDOW_HOURS="${WINDOW_HOURS:-48}"
MIN_EMBED_PCT="${MIN_EMBED_PCT:-90}"
JSON_ONLY="${1:-}"

TOKEN="$(grep '^XAVIER_TOKEN=' "$TOKEN_FILE" 2>/dev/null | cut -d= -f2- | tr -d '"' | tr -d "'")"
[ -n "$TOKEN" ] || { echo '{"status":"error","check":"token","detail":"no se pudo leer XAVIER_TOKEN"}' >&2; exit 1; }

# --- (a) servidor vivo -------------------------------------------------------
health_json="$(curl -s --max-time 10 "$XAVIER_URL/health" 2>/dev/null)"
server_ok="no"; db_status="unknown"
if echo "$health_json" | grep -q '"status":"healthy"\|"integrity_ok":true'; then
  server_ok="yes"
  db_status="$(echo "$health_json" | python3 -c "import sys,json; print(json.load(sys.stdin).get('database',{}).get('status','unknown'))" 2>/dev/null)"
fi

# --- (b) escrituras nuevas en la ventana --------------------------------------
recent=0
if [ -f "$VEC_DB" ]; then
  recent="$(python3 -c "
import sqlite3, sys
c = sqlite3.connect('$VEC_DB')
try:
    print(c.execute(\"SELECT COUNT(*) FROM memory_records WHERE created_at >= datetime('now','localtime','-$WINDOW_HOURS hours')\").fetchone()[0])
except Exception:
    print(0)
" 2>/dev/null || echo 0)"
fi

# --- (c) cobertura de embeddings ----------------------------------------------
tot=0; emb=0; pct=0
if [ -f "$VEC_DB" ]; then
  read -r tot emb <<<"$(python3 -c "
import sqlite3
c = sqlite3.connect('$VEC_DB')
try:
    t = c.execute('SELECT COUNT(*) FROM memory_records').fetchone()[0]
    e = c.execute('SELECT COUNT(*) FROM memory_records WHERE length(embedding)>10').fetchone()[0]
    print(t, e)
except Exception:
    print('0 0')
" 2>/dev/null || echo "0 0")"
  if [ "${tot:-0}" -gt 0 ] 2>/dev/null; then
    pct=$(( emb * 100 / tot ))
  fi
fi

# --- (d) recall semántico vivo (σ>0) ------------------------------------------
recall_ok="no"; recall_n=0
probe_q="$(curl -s --max-time 15 -X POST "$XAVIER_URL/memory/search" \
  -H "X-Xavier-Token: $TOKEN" -H 'Content-Type: application/json' \
  -d '{"query":"Xavier memory health probe 2026","limit":3}' 2>/dev/null)"
if [ -n "$probe_q" ]; then
  recall_n="$(echo "$probe_q" | python3 -c "
import sys,json
try:
    d=json.load(sys.stdin)
    results=d.get('results',[])
    scored=[r for r in results if (r.get('score') or 0) > 0]
    print(len(scored))
except Exception:
    print(0)
" 2>/dev/null)"
  [ "${recall_n:-0}" -gt 0 ] 2>/dev/null && recall_ok="yes"
fi

# --- evaluación ----------------------------------------------------------------
fail=""
[ "$server_ok" = "yes" ] || fail="${fail} server_down"
[ "$db_status" = "healthy" ] || fail="${fail} db_unhealthy"
[ "${recent:-0}" -gt 0 ] || fail="${fail} no_recent_writes"
[ "$pct" -ge "$MIN_EMBED_PCT" ] || fail="${fail} low_embedding_coverage(${pct}%)"
[ "$recall_ok" = "yes" ] || fail="${fail} recall_no_scores"

if [ -n "$fail" ]; then
  status="fail"
  exit_code=1
else
  status="ok"
  exit_code=0
fi

python3 -c "
import json,sys
print(json.dumps({
  'status': '$status',
  'ts': __import__('datetime').datetime.now().isoformat(),
  'checks': {
    'server': '$server_ok',
    'db': '$db_status',
    'recent_writes_${WINDOW_HOURS}h': $recent,
    'embedding_coverage_pct': $pct,
    'recall_scored': '$recall_ok',
  },
  'failures': '$fail'.strip().split(),
}, indent=2))
"

exit $exit_code
