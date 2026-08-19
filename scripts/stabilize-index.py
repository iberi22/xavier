#!/usr/bin/env python3
"""stabilize-index — SSP-OlaB (#1234): indexa 1 snippet por feature de features.json
de un repo SWAL en Xavier como feature_snippet (path features/{repo}/{feature_id}).

Uso: stabilize-index.py <repo-path> [xavier-url]
Ej.: stabilize-index.py ~/proyectosSWAL/shelf http://localhost:8006
"""
import json
import os
import sys
import urllib.request

repo_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/proyectosSWAL/shelf")
xavier_url = sys.argv[2] if len(sys.argv) > 2 else "http://localhost:8006"
features_json = os.path.join(repo_dir, ".gitcore", "features.json")
repo_slug = os.path.basename(os.path.normpath(repo_dir))

if not os.path.exists(features_json):
    print(f"ERROR: no existe {features_json}")
    sys.exit(1)

token = ""
token_file = os.environ.get("XAVIER_TOKEN_FILE", "/tmp/xavier-token.txt")
if os.path.exists(token_file):
    token = open(token_file).read().strip()

headers = {"Content-Type": "application/json"}
if token:
    headers["X-Xavier-Token"] = token

def post(path, payload):
    req = urllib.request.Request(
        f"{xavier_url}{path}",
        data=json.dumps(payload).encode(),
        headers=headers,
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return json.load(r)
    except Exception as e:
        print(f"  POST {path} ERROR: {e}")
        return None

d = json.load(open(features_json))
feats = d.get("features", d) if isinstance(d, dict) else d
if isinstance(feats, dict):
    items = list(feats.items())
else:
    items = [(f.get("id", f.get("name", "?")), f) for f in feats]

ok = 0
for fid, f in items:
    pp = f.get("progress_pct", f.get("progress", 0))
    try:
        pp = float(pp)
    except Exception:
        pp = 0.0
    status = f.get("status", "")
    last_tested = f.get("last_tested", f.get("last_verified", ""))
    implemented_in = f.get("implemented_in", "")
    req_ids = f.get("req_ids", [])
    if isinstance(req_ids, list):
        req_ids = ",".join(str(x) for x in req_ids)
    tests = f.get("tests", "")
    snippet = f"{fid} %real={pp:.0f} status={status} tested={last_tested} paths={implemented_in} reqs={req_ids} tests={tests}"
    snippet = snippet[:290]
    path = f"features/{repo_slug}/{fid}"
    resp = post("/v1/memories", {
        "content": snippet,
        "path": path,
        "metadata": {
            "kind": "feature_snippet",
            "provenance": {"project_root": repo_dir},
        },
    })
    if resp:
        ok += 1
        print(f"OK  {path}")
    else:
        print(f"FAIL {path}")

print(f"stabilize-index {repo_slug}: {ok}/{len(items)} snippets indexados")
