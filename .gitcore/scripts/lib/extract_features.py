import json, sys
sys.path.insert(0, "/app/.gitcore/scripts/lib")
from load_lenient import load_lenient
d = load_lenient(sys.argv[1])
features_raw = d["features"]
# soporta formato dict (keyed) y lista
items = features_raw.values() if isinstance(features_raw, dict) else features_raw
for f in items:
    reqs = ",".join(f.get("req_ids", []))
    us = ",".join(f.get("user_stories", []))
    impl = f.get("implemented_in", "") or ""
    tests = ",".join(f.get("tests", [])) if isinstance(f.get("tests"), list) else str(f.get("tests", ""))
    print(f'{f["id"]}|{f.get("progress_pct",0)}|{reqs}|{us}|{impl}|{tests}')
