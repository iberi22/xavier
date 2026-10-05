#!/usr/bin/env python3
"""Base vs candidate evaluation for Xavier mini-experts (MX-05).

Runs the SAME fixed eval.jsonl against two Ollama models through the HTTP API and
prints PROMOTE or REJECT. Standard library only.

Metrics: exact match, normalized match, token-F1, p50/p95 latency, optional rubric
judge (--judge-cmd: executable that reads {"prompt","expected","answer"} JSON on stdin
and prints a float in [0,1]; off by default).

Promote iff  score(candidate) >= score(base) + margin
         and p95(candidate)  <= p95(base) * latency_budget.
Exit codes: 0 PROMOTE, 2 REJECT, 1 error (model unreachable, bad eval file...).
"""
import argparse
import json
import re
import shlex
import statistics
import string
import subprocess
import sys
import time
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "training"))
from expert_core import ExpertError, extract_pair, read_pairs  # noqa: E402

_PUNCT = str.maketrans({c: " " for c in string.punctuation})


def normalize(s):
    return re.sub(r"\s+", " ", s.lower().translate(_PUNCT)).strip()


def token_f1(pred, ref):
    p, r = normalize(pred).split(), normalize(ref).split()
    if not p or not r:
        return 1.0 if p == r else 0.0
    common = sum((Counter(p) & Counter(r)).values())
    if common == 0:
        return 0.0
    prec, rec = common / len(p), common / len(r)
    return 2 * prec * rec / (prec + rec)


def percentile(values, q):
    if not values:
        return None
    s = sorted(values)
    k = (len(s) - 1) * q
    lo, hi = int(k), min(int(k) + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


def ollama_chat(url, model, prompt, num_predict, timeout):
    body = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": False,
        "options": {"temperature": 0, "seed": 42, "num_predict": num_predict},
    }).encode()
    req = urllib.request.Request(url.rstrip("/") + "/api/chat", data=body,
                                 headers={"Content-Type": "application/json"}, method="POST")
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        data = json.loads(resp.read())
    dt = time.perf_counter() - t0
    return data.get("message", {}).get("content", ""), dt


def judge(cmd, prompt, expected, answer):
    r = subprocess.run(shlex.split(cmd), input=json.dumps(
        {"prompt": prompt, "expected": expected, "answer": answer}), capture_output=True,
        text=True, timeout=120)
    if r.returncode != 0:
        raise ExpertError("judge command failed: %s" % r.stderr.strip()[:200])
    try:
        return max(0.0, min(1.0, float(r.stdout.strip())))
    except ValueError:
        raise ExpertError("judge must print a float, got %r" % r.stdout[:50])


def eval_model(args, model, pairs):
    exact = norm = f1 = judged = 0.0
    lat, errors = [], 0
    for prompt, expected in pairs:
        try:
            answer, dt = ollama_chat(args.ollama_url, model, prompt, args.num_predict, args.timeout)
        except (urllib.error.URLError, OSError, ValueError) as e:
            errors += 1
            if errors == 1 and not lat:
                raise ExpertError("cannot query model %r at %s: %s" % (model, args.ollama_url, e))
            continue
        lat.append(dt)
        exact += answer.strip() == expected.strip()
        norm += normalize(answer) == normalize(expected)
        f1 += token_f1(answer, expected)
        if args.judge_cmd:
            judged += judge(args.judge_cmd, prompt, expected, answer)
    n = len(pairs)
    res = {
        "model": model, "n": n, "errors": errors,
        "exact": exact / n, "normalized": norm / n, "f1": f1 / n,
        "latency_p50_s": percentile(lat, 0.5), "latency_p95_s": percentile(lat, 0.95),
        "latency_mean_s": statistics.mean(lat) if lat else None,
    }
    if args.judge_cmd:
        res["judge"] = judged / n
    return res


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--eval", required=True, help="eval.jsonl (the fixed split from the bundle)")
    ap.add_argument("--base", required=True, help="Ollama base model name")
    ap.add_argument("--candidate", required=True, help="Ollama candidate model name")
    ap.add_argument("--ollama-url", default="http://127.0.0.1:11434")
    ap.add_argument("--metric", choices=("f1", "normalized", "exact", "judge"), default="f1")
    ap.add_argument("--margin", type=float, default=0.05, help="required score gain over base")
    ap.add_argument("--latency-budget", type=float, default=2.0,
                    help="candidate p95 must be <= base p95 * this (0 disables)")
    ap.add_argument("--judge-cmd", default=None, help="optional rubric judge command (off by default)")
    ap.add_argument("--num-predict", type=int, default=256)
    ap.add_argument("--timeout", type=float, default=300)
    ap.add_argument("--limit", type=int, default=0, help="evaluate only the first N examples")
    ap.add_argument("--report", default="eval_report.json")
    args = ap.parse_args(argv)
    try:
        if args.metric == "judge" and not args.judge_cmd:
            raise ExpertError("--metric judge requires --judge-cmd")
        pairs = read_pairs(args.eval)
        if args.limit:
            pairs = pairs[: args.limit]
        if not pairs:
            raise ExpertError("eval file has no usable examples")
        if len(pairs) < 20:
            print("WARNING: only %d eval examples; result is statistically weak" % len(pairs), file=sys.stderr)
        base = eval_model(args, args.base, pairs)
        cand = eval_model(args, args.candidate, pairs)
    except ExpertError as e:
        print("ERROR: %s" % e, file=sys.stderr)
        return 1
    key = args.metric
    gain = cand[key] - base[key]
    reasons = []
    if gain < args.margin:
        reasons.append("score gain %.4f < margin %.4f" % (gain, args.margin))
    bp, cp = base["latency_p95_s"], cand["latency_p95_s"]
    if args.latency_budget > 0 and bp and cp and cp > bp * args.latency_budget:
        reasons.append("p95 latency %.2fs > %.2fx base %.2fs" % (cp, args.latency_budget, bp))
    if cand["errors"] > base["errors"]:
        reasons.append("candidate had more request errors than base")
    verdict = "REJECT" if reasons else "PROMOTE"
    report = {"verdict": verdict, "metric": key, "margin": args.margin, "gain": gain,
              "latency_budget": args.latency_budget, "reasons": reasons, "base": base,
              "candidate": cand, "eval_file": str(args.eval)}
    Path(args.report).write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps({k: report[k] for k in ("metric", "gain", "reasons", "base", "candidate")}, indent=2))
    print(verdict)
    return 0 if verdict == "PROMOTE" else 2


if __name__ == "__main__":
    sys.exit(main())
