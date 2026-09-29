#!/usr/bin/env python3
"""Index every benchmark PDF through MCP and record builder/node stats."""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pi import call
DOCS = sys.argv[1]
rows = []
for fn in sorted(os.listdir(DOCS)):
    if not fn.endswith(".pdf"): continue
    t = time.time()
    r = call("pageindex_index_document", {"doc_name": fn, "path": os.path.join(DOCS, fn), "format": "pdf"})
    dt = time.time() - t
    d = call("pageindex_get_document", {"doc_name": fn})
    rows.append({"doc": fn, "secs": round(dt, 2), "ingest": r, "doc_info": d})
    print(fn, round(dt, 1), json.dumps(d)[:300], flush=True)
json.dump(rows, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "ingest_results.json"), "w"), indent=1)
