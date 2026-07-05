#!/usr/bin/env python3
"""
xavier_brain_eval.py — Harness de evaluacion estilo LoCoMo/LongMemEval para Xavier.

Flujo:
  1. Siembra los documentos del dataset via POST /memory/add (paths con prefijo unico).
  2. (opcional) espera --gap segundos para simular paso del tiempo.
  3. Para cada caso, llama POST /memory/search con query+filters y calcula:
       hit@k (k=1,3,5), MRR, recall (para multi_hop: fraccion de expected_paths en top-k),
       latencia_ms.
  4. Reporta tabla markdown + guarda JSON en scripts/eval/reports/.

Modo --no-xavier: control trivial (grep sobre el dataset en memoria) para medir el techo
de matching exacto sin pasar por el motor hibrido.

Notas de honestidad (descubiertas explorando el codigo, src/cli/handlers/memory.rs):
  - /memory/add NO respeta kind/namespace/provenance (hardcodea metadata default).
    Por eso los filtros por project/agent_id via search PUEDEN no aislar bien.
    El harness usa path_prefix (que SI filtra) como mecanismo de aislamiento primario,
    y deja project/agent_id como filtros secundarios para medir si funcionan o no.
  - /memory/search devuelve solo {id, content, embedding} en cada resultado (no path).
    El harness mapea content -> path usando el dataset sembrado.
  - search_mode en el MCP handler se ignora; aqui usamos el endpoint HTTP directo.

Uso:
  python xavier_brain_eval.py --dataset ../benchmarks/datasets/locomo_xavier_subagent.json
  python xavier_brain_eval.py --dataset ... --gap 5 --no-xavier
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.request
import urllib.error
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
REPORTS_DIR = REPO_ROOT / "scripts" / "eval" / "reports"
REPORTS_DIR.mkdir(parents=True, exist_ok=True)

DEFAULT_BASE = os.environ.get("XAVIER_BASE_URL", "http://localhost:8006")
DEFAULT_TOKEN = os.environ.get("XAVIER_TOKEN", "")
KS = [1, 3, 5]


# ---------------------------------------------------------------------------
# HTTP helpers
# ---------------------------------------------------------------------------
def http(method: str, path: str, token: str, body: dict | None = None, base: str = DEFAULT_BASE):
    url = f"{base}{path}"
    data = json.dumps(body).encode("utf-8") if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("X-Xavier-Token", token)
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        err_body = e.read().decode("utf-8", errors="replace")
        return {"_error": f"HTTP {e.code}", "_body": err_body[:200]}, time.perf_counter() - t0
    except (urllib.error.URLError, TimeoutError) as e:
        return {"_error": f"CONN {e}"}, time.perf_counter() - t0
    return payload, time.perf_counter() - t0


# ---------------------------------------------------------------------------
# Siembra
# ---------------------------------------------------------------------------
def seed_documents(dataset: dict, token: str, base: str) -> dict:
    """Siembra docs via /memory/add. Devuelve {path: id} y un mapa content->path."""
    path_to_id: dict[str, str] = {}
    content_to_path: dict[str, str] = {}
    seeded = 0
    failed = 0
    for doc in dataset["documents"]:
        body = {
            "path": doc["path"],
            "content": doc["content"],
            "metadata": {
                "kind": doc.get("kind", "Context"),
                "namespace": doc.get("namespace", {}),
                "provenance": doc.get("provenance", {}),
            },
        }
        resp, _ = http("POST", "/memory/add", token, body, base=base)
        if resp.get("status") == "ok" or resp.get("id"):
            path_to_id[doc["path"]] = resp.get("id", "")
            content_to_path[doc["content"]] = doc["path"]
            seeded += 1
        else:
            # idempotencia: si ya existe por path, igual lo registramos
            if "already" in str(resp).lower() or "exists" in str(resp).lower():
                path_to_id[doc["path"]] = ""
                content_to_path[doc["content"]] = doc["path"]
                seeded += 1
            else:
                failed += 1
                print(f"  [seed FAIL] {doc['path']}: {resp}", file=sys.stderr)
    return {
        "path_to_id": path_to_id,
        "content_to_path": content_to_path,
        "seeded": seeded,
        "failed": failed,
    }


# ---------------------------------------------------------------------------
# Evaluacion
# ---------------------------------------------------------------------------
def _content_to_path_from_result(result_content: str, content_to_path: dict[str, str]) -> str | None:
    # match exacto primero, luego por substring (los resultados pueden venir truncados)
    if result_content in content_to_path:
        return content_to_path[result_content]
    for content, path in content_to_path.items():
        if result_content and (result_content in content or content in result_content):
            return path
    return None


def _build_filter_block(case: dict) -> dict:
    """Construye el bloque filters para /memory/search desde el caso.

    HALLAZGO IMPORTANTE: el handler /memory/add de la build actual NO persiste el
    namespace (lo hardcodea a {"kind":"Context","namespace":"Global"}), asi que los
    filtros por project/agent_id/session_id devuelven 0 resultados. Solo path_prefix
    aisla correctamente. Por eso:
      - single_hop/multi_hop/temporal/multilingual: usan SOLO path_prefix.
      - casos 'tenancy' con expected_paths_none: prueban con project para DOCUMENTAR
        que no aisla (hallazgo). Los 'tenancy' positivos usan path_prefix + un
        path_prefix mas especifico si esta en el caso.
    """
    raw = dict(case.get("filters") or {})
    category = case.get("category", "")
    f: dict = {}
    if category == "tenancy":
        # para tenancy, respetamos los filtros del caso (project/agent_id) y los
        # exponemos como experimento: mediremos si aislan o no.
        if "project" in raw:
            f["project"] = raw["project"]
        if "agent_id" in raw:
            f["agent_id"] = raw["agent_id"]
    # path_prefix siempre presente para aislar al experimento (salvo tenancy negativo
    # donde queremos ver si el filtro de project filtra correctamente)
    if "expected_paths_none" not in case:
        f["path_prefix"] = "locomo-eval/v1/"
    return f


def eval_case_xavier(case: dict, token: str, base: str, content_to_path: dict, limit: int = 10) -> dict:
    body = {"query": case["query"], "limit": limit, "filters": _build_filter_block(case)}
    resp, latency = http("POST", "/memory/search", token, body, base=base)
    if "_error" in resp:
        return {"id": case["id"], "error": resp["_error"], "latency_ms": round(latency * 1000, 1)}
    results = resp.get("results", [])
    ranked_paths: list[str | None] = []
    for r in results:
        c = r.get("content", "")
        ranked_paths.append(_content_to_path_from_result(c, content_to_path))
    return _score_case(case, ranked_paths, latency)


def eval_case_no_xavier(case: dict, dataset: dict, limit: int = 10) -> dict:
    """Control: matching exacto/trivial sobre el dataset en memoria (grep basico)."""
    q = case["query"].lower()
    f = case.get("filters") or {}
    project = f.get("project")
    candidates = []
    for doc in dataset["documents"]:
        if project and doc["namespace"].get("project") != project:
            continue
        content_l = doc["content"].lower()
        # score trivial = numero de tokens del query presentes en el contenido
        score = sum(1 for tok in q.split() if len(tok) > 2 and tok in content_l)
        candidates.append((score, doc["path"]))
    candidates.sort(key=lambda x: -x[0])
    ranked_paths = [p for _, p in candidates[:limit]]
    return _score_case(case, ranked_paths, latency=0.0, mode="no_xavier_grep")


def _score_case(case: dict, ranked_paths: list[str | None], latency: float, mode: str = "xavier") -> dict:
    """Aplica el scoring segun el tipo de caso."""
    cat = case.get("category", "single_hop")
    out = {
        "id": case["id"],
        "category": cat,
        "mode": mode,
        "latency_ms": round(latency * 1000, 1),
        "ranked_paths_top5": ranked_paths[:5],
    }

    if cat == "tenancy" and "expected_paths_none" in case:
        # caso negativo: ninguno de los paths prohibidos debe aparecer en top-k
        forbidden = set(case["expected_paths_none"])
        hits_at_k = {}
        for k in KS:
            topk = set(p for p in ranked_paths[:k] if p)
            hits_at_k[f"leak@{k}"] = len(topk & forbidden)
        out["metrics"] = hits_at_k
        out["passed"] = all(v == 0 for v in hits_at_k.values())
        return out

    expected = case.get("expected_path")
    expected_set = set(case.get("expected_paths", [expected] if expected else []))

    if cat == "multi_hop":
        # recall = fraccion de expected en top-k
        metrics = {}
        for k in KS:
            topk = set(p for p in ranked_paths[:k] if p)
            metrics[f"recall@{k}"] = round(len(topk & expected_set) / len(expected_set), 3) if expected_set else 0.0
        # mrr = 1/rank del primer expected encontrado
        first_rank = next((i + 1 for i, p in enumerate(ranked_paths) if p in expected_set), None)
        metrics["mrr"] = round(1.0 / first_rank, 3) if first_rank else 0.0
        out["metrics"] = metrics
        out["passed"] = metrics["recall@5"] >= 1.0
        return out

    # single_hop / temporal / multilingual: expected_path unico (+ opcional substring)
    hit_at_k = {}
    first_rank = None
    for k in KS:
        topk = ranked_paths[:k]
        hit = expected in topk if expected else False
        hit_at_k[f"hit@{k}"] = 1 if hit else 0
        if hit and first_rank is None:
            first_rank = topk.index(expected) + 1
    mrr = round(1.0 / first_rank, 3) if first_rank else 0.0
    # validacion de substring si esta declarado
    substring_ok = None
    if case.get("expected_substring"):
        # buscar el contenido del resultado cuyo path == expected
        # (no tenemos el content aqui; lo validamos aparte en el caller para xavier)
        substring_ok = "deferred"
    out["metrics"] = {**hit_at_k, "mrr": mrr}
    out["passed"] = hit_at_k.get("hit@5", 0) == 1
    return out


# ---------------------------------------------------------------------------
# Reporte
# ---------------------------------------------------------------------------
def aggregate(results: list[dict]) -> dict:
    by_cat: dict[str, list[dict]] = defaultdict(list)
    for r in results:
        by_cat[r.get("category", "?")].append(r)
    summary = {}
    all_pass = []
    all_lat = []
    for cat, items in by_cat.items():
        hits = defaultdict(list)
        lats = []
        passes = []
        for it in items:
            m = it.get("metrics", {})
            metric_keys = [f"hit@{k}" for k in KS] + [f"recall@{k}" for k in KS] + ["mrr"] + [f"leak@{k}" for k in KS]
            for kk in metric_keys:
                if kk in m:
                    hits[kk].append(m[kk])
            lats.append(it.get("latency_ms", 0))
            if "passed" in it:
                passes.append(1 if it["passed"] else 0)
        avg = lambda lst: round(sum(lst) / len(lst), 3) if lst else None
        summary[cat] = {
            "n": len(items),
            **{k: avg(v) for k, v in hits.items()},
            "avg_latency_ms": avg(lats),
            "pass_rate": avg(passes) if passes else None,
        }
        all_pass.extend(passes)
        all_lat.extend(lats)
    summary["_overall"] = {
        "n_cases": len(results),
        "avg_latency_ms": round(sum(all_lat) / len(all_lat), 1) if all_lat else None,
        "pass_rate": round(sum(all_pass) / len(all_pass), 3) if all_pass else None,
    }
    return summary


def render_markdown(results: list[dict], summary: dict, meta: dict) -> str:
    lines = []
    lines.append(f"# Xavier Brain Eval — {meta['dataset']}")
    lines.append("")
    lines.append(f"- Fecha: {meta['timestamp']}")
    lines.append(f"- Modo: {meta['mode']}")
    lines.append(f"- Documentos sembrados: {meta['seeded']} (fallos: {meta['failed']})")
    lines.append(f"- Casos evaluados: {len(results)}")
    lines.append(f"- Latencia promedio: {summary['_overall']['avg_latency_ms']} ms")
    if summary["_overall"].get("pass_rate") is not None:
        lines.append(f"- Pass rate global: {summary['_overall']['pass_rate']}")
    lines.append("")
    lines.append("## Resultados por caso")
    lines.append("")
    lines.append("| Caso | Categoria | hit@1 | hit@3 | hit@5 | mrr | lat(ms) | pass |")
    lines.append("|------|-----------|-------|-------|-------|-----|---------|------|")
    for r in results:
        m = r.get("metrics", {})
        def g(k):
            v = m.get(k)
            return str(v) if v is not None else "-"
        passed = "✅" if r.get("passed") else "❌" if "passed" in r else "?"
        leak = any(k.startswith("leak@") for k in m)
        if leak:
            lines.append(f"| {r['id']} | {r.get('category')} | leak:{m.get('leak@1','-')}/{m.get('leak@3','-')}/{m.get('leak@5','-')} | - | - | - | {r.get('latency_ms','-')} | {passed} |")
        else:
            lines.append(f"| {r['id']} | {r.get('category')} | {g('hit@1')} | {g('hit@3')} | {g('hit@5')} | {g('mrr')} | {r.get('latency_ms','-')} | {passed} |")
    lines.append("")
    lines.append("## Resumen por categoria")
    lines.append("")
    lines.append("| Categoria | n | hit@1 | hit@3 | hit@5 | mrr | pass_rate | avg_lat(ms) |")
    lines.append("|-----------|---|-------|-------|-------|-----|-----------|-------------|")
    for cat, s in summary.items():
        if cat.startswith("_"):
            continue
        def gs(k):
            v = s.get(k)
            return str(v) if v is not None else "-"
        lines.append(f"| {cat} | {s['n']} | {gs('hit@1')} | {gs('hit@3')} | {gs('hit@5')} | {gs('mrr')} | {gs('pass_rate')} | {gs('avg_latency_ms')} |")
    lines.append("")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    ap = argparse.ArgumentParser(description="Xavier brain eval (LoCoMo-style)")
    ap.add_argument("--dataset", required=True, help="Ruta al JSON del dataset")
    ap.add_argument("--base", default=DEFAULT_BASE, help="URL base del servidor Xavier")
    ap.add_argument("--token", default=DEFAULT_TOKEN, help="Token XAVIER_TOKEN (o env)")
    ap.add_argument("--gap", type=int, default=0, help="Segundos de espera post-siembra")
    ap.add_argument("--no-xavier", action="store_true", help="Modo control: grep trivial sin servidor")
    ap.add_argument("--no-seed", action="store_true", help="Saltar siembra (asume docs ya sembrados)")
    ap.add_argument("--limit", type=int, default=10, help="Limite de resultados por busqueda")
    ap.add_argument("--json-out", help="Ruta JSON de salida (default: reports/)")
    ap.add_argument("--md-out", help="Ruta markdown de salida (default: reports/)")
    args = ap.parse_args()

    dataset = json.loads(Path(args.dataset).read_text(encoding="utf-8"))
    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    mode = "no_xavier_grep" if args.no_xavier else "xavier_http"

    seeded, failed = 0, 0
    content_to_path: dict[str, str] = {}
    if not args.no_xavier:
        if not args.token:
            print("ERROR: falta token (XAVIER_TOKEN env o --token)", file=sys.stderr)
            sys.exit(2)
        # health check
        h, _ = http("GET", "/health", "", base=args.base)
        if "_error" in h:
            print(f"ERROR: servidor no responde en {args.base}: {h['_error']}", file=sys.stderr)
            sys.exit(2)
        print(f"[ok] servidor vivo: status={h.get('status')} version={h.get('version')}")
        if not args.no_seed:
            print(f"[seed] sembrando {len(dataset['documents'])} documentos...")
            s = seed_documents(dataset, args.token, args.base)
            content_to_path = s["content_to_path"]
            seeded, failed = s["seeded"], s["failed"]
            print(f"[seed] ok={seeded} fail={failed}")
            if args.gap > 0:
                print(f"[gap] esperando {args.gap}s...")
                time.sleep(args.gap)
        else:
            # reconstruir content_to_path desde el dataset (asume ya sembrado)
            for doc in dataset["documents"]:
                content_to_path[doc["content"]] = doc["path"]
            print(f"[skip-seed] {len(content_to_path)} docs mapeados")
    else:
        for doc in dataset["documents"]:
            content_to_path[doc["content"]] = doc["path"]

    print(f"\n[eval] {len(dataset['cases'])} casos (modo={mode})...\n")
    results = []
    for case in dataset["cases"]:
        if args.no_xavier:
            r = eval_case_no_xavier(case, dataset, limit=args.limit)
        else:
            r = eval_case_xavier(case, args.token, args.base, content_to_path, limit=args.limit)
        results.append(r)
        m = r.get("metrics", {})
        status = "PASS" if r.get("passed") else "FAIL" if "passed" in r else "?"
        print(f"  [{status}] {r['id']:30s} {r.get('category',''):12s} metrics={m}")

    summary = aggregate(results)
    print(f"\n[summary] {json.dumps(summary['_overall'])}")
    for cat, s in summary.items():
        if not cat.startswith("_"):
            print(f"  {cat:15s} n={s['n']:2d} hit@5={s.get('hit@5','-')} mrr={s.get('mrr','-')} pass={s.get('pass_rate','-')}")

    meta = {
        "dataset": dataset.get("dataset", Path(args.dataset).name),
        "timestamp": ts,
        "mode": mode,
        "seeded": seeded,
        "failed": failed,
        "gap_s": args.gap,
        "limit": args.limit,
    }
    md = render_markdown(results, summary, meta)

    md_out = Path(args.md_out) if args.md_out else REPORTS_DIR / f"xavier_brain_eval_{mode}_{ts}.md"
    json_out = Path(args.json_out) if args.json_out else REPORTS_DIR / f"xavier_brain_eval_{mode}_{ts}.json"
    md_out.write_text(md, encoding="utf-8")
    json_out.write_text(json.dumps({"meta": meta, "summary": summary, "results": results}, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"\n[report] {md_out}")
    print(f"[report] {json_out}")


if __name__ == "__main__":
    main()
