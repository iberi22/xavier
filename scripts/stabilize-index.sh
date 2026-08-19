#!/usr/bin/env bash
# stabilize-index — SSP-OlaB (#1234): indexa 1 snippet por feature de features.json
# de un repo SWAL en Xavier como kind=feature_snippet (path features/{repo}/{feature_id}).
# Uso: stabilize-index.sh <repo-path> [xavier-url]
# Ej.: stabilize-index.sh ~/proyectosSWAL/shelf http://localhost:8006
set -euo pipefail

REPO_DIR="${1:-}"
XAVIER_URL="${2:-http://localhost:8006}"
FEATURES_JSON="$REPO_DIR/.gitcore/features.json"

if [ ! -f "$FEATURES_JSON" ]; then
  echo "ERROR: no existe $FEATURES_JSON" >&2
  exit 1
fi

REPO_SLUG="$(basename "$REPO_DIR")"
TOKEN_FILE="${XAVIER_TOKEN_FILE:-/tmp/xavier-token.txt}"
AUTH_HEADER=()
if [ -f "$TOKEN_FILE" ]; then
  AUTH_HEADER=(-H "X-Xavier-Token: $(cat "$TOKEN_FILE")")
fi

if ! curl -s -m 3 "$XAVIER_URL/health" >/dev/null 2>&1; then
  echo "ERROR: Xavier no responde en $XAVIER_URL" >&2
  exit 2
fi

python3 - "$REPO_DIR" "$REPO_SLUG" <<'PYEOF'
import json, os, sys

repo_dir, repo_slug = sys.argv[1], sys.argv[2]
d = json.load(open(os.path.join(repo_dir, '.gitcore', 'features.json')))
feats = d.get('features', d) if isinstance(d, dict) else d
items = list(feats.items()) if isinstance(feats, dict) else [(f.get('id', f.get('name', '?')), f) for f in feats]
for fid, f in items:
    pp = f.get('progress_pct', f.get('progress', 0))
    try:
        pp = float(pp)
    except Exception:
        pp = 0.0
    status = f.get('status', '')
    last_tested = f.get('last_tested', f.get('last_verified', ''))
    implemented_in = f.get('implemented_in', '')
    req_ids = f.get('req_ids', [])
    if isinstance(req_ids, list):
        req_ids = ','.join(str(x) for x in req_ids)
    tests = f.get('tests', '')
    snippet = f"{fid} %real={pp:.0f} status={status} tested={last_tested} paths={implemented_in} reqs={req_ids} tests={tests}"
    print(f"{fid}\t{snippet[:290]}")
PYEOF
