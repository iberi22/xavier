#!/usr/bin/env python3
"""Classic-RAG baseline: BM25 over pages (same page text Xavier extracted, fetched via MCP)."""
import json, math, os, re, sys
from collections import Counter
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pi import call
HERE = os.path.dirname(os.path.abspath(__file__))
qs = json.load(open(sys.argv[1]))
cache_p = os.path.join(HERE, "pages_cache.json")
cache = json.load(open(cache_p)) if os.path.exists(cache_p) else {}
def pages_of(doc, n):
    if doc in cache: return cache[doc]
    out = {}
    todo = [f"{a}-{min(n, a + 19)}" for a in range(1, n + 1, 20)]
    while todo:
        spec = todo.pop(0)
        body = call("pageindex_get_page_content", {"doc_name": doc, "pages": spec}).get("structuredContent", {})
        if not body.get("ok"): break
        for pg in body.get("pages", []):
            out[str(pg["page_no"])] = out.get(str(pg["page_no"]), "") + pg["text"]
        if body.get("remaining_pages"): todo.insert(0, body["remaining_pages"])
    cache[doc] = out
    json.dump(cache, open(cache_p, "w"))
    return out
tok = lambda s: re.findall(r"[a-z0-9]+", s.lower())
def bm25(q, pages, k1=1.5, b=0.75):
    docs = {p: tok(t) for p, t in pages.items()}
    N = len(docs); avg = sum(map(len, docs.values())) / max(N, 1)
    df = Counter(w for d in docs.values() for w in set(d))
    qt = tok(q); sc = {}
    for p, d in docs.items():
        tf = Counter(d); s = 0.0
        for w in qt:
            if w in tf:
                idf = math.log(1 + (N - df[w] + .5) / (df[w] + .5))
                s += idf * tf[w] * (k1 + 1) / (tf[w] + k1 * (1 - b + b * len(d) / avg))
        sc[p] = s
    return [int(p) for p, _ in sorted(sc.items(), key=lambda x: -x[1])]
def page_counts(rows):
    """Map ingested doc name -> page count from ingest.py rows {doc, doc_info}."""
    out = {}
    for r in rows:
        sc = (r.get("doc_info") or {}).get("structuredContent") or {}
        if sc.get("ok") and "page_count" in sc: out[r["doc"]] = sc["page_count"]
    return out
def resolve(doc_id, info):
    """Benchmark doc_id may omit the .pdf suffix that ingest.py used as the doc name."""
    for name in (doc_id, doc_id + ".pdf"):
        if name in info: return name
    raise SystemExit(f"doc {doc_id!r} not found in {sys.argv[2]} (ingest it first)")
info = page_counts(json.load(open(sys.argv[2])))
res = []
for q in qs:
    name = resolve(q["doc_id"], info)
    pages = pages_of(name, info[name])
    rank = bm25(q["question"], pages)
    gold = set(json.loads(q["evidence_pages"]))
    res.append({"q": q["question"], "gold": sorted(gold), "top5": rank[:5],
                "hit1": bool(gold & set(rank[:1])), "hit3": bool(gold & set(rank[:3])), "hit5": bool(gold & set(rank[:5])),
                "pages_extracted": len(pages)})
n = len(res)
print(json.dumps({k: round(sum(r[k] for r in res) / n, 3) for k in ("hit1", "hit3", "hit5")} | {"n": n}))
json.dump(res, open(os.path.join(HERE, "bm25_results.json"), "w"), indent=1)
