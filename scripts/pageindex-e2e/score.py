#!/usr/bin/env python3
"""Score navigator runs against the benchmark key (run by the orchestrator only)."""
import json, os, re, glob, unicodedata
# Usage: score.py <questions.json>  (or env BENCH_QUESTIONS); run from the directory holding nav_answers_*.json and nav_logs/.
import sys
qs = json.load(open(sys.argv[1] if len(sys.argv) > 1 else os.environ["BENCH_QUESTIONS"]))
ans = {}
for f in glob.glob('nav_answers_*.json'):
    for a in json.load(open(f)): ans[a['qid']] = a
def spec_pages(spec):
    out = set()
    for part in str(spec).split(','):
        part = part.strip()
        if not part: continue
        if '-' in part:
            a, b = part.split('-', 1); out.update(range(int(a), int(b) + 1))
        else: out.add(int(part))
    return out
norm = lambda s: re.sub(r'[^a-z0-9%.]+', ' ', unicodedata.normalize('NFKD', str(s)).lower()).strip()
rows = []
for i, q in enumerate(qs):
    gold = set(json.loads(q['evidence_pages']))
    read, calls = set(), 0
    for lp in [p for p in glob.glob('nav_logs/q*') if re.match(rf'q{i}(?!\d)', os.path.basename(p))]:
        for line in open(lp):
            e = json.loads(line); calls += 1
            if e['tool'] == 'pageindex_get_page_content': read |= spec_pages(e['args'].get('pages', ''))
    a = ans.get(i, {})
    ra, ref = norm(a.get('answer', '')), norm(q['answer'])
    auto = bool(ref) and (ref in ra or (ra and ra in ref and len(ra) > 3))
    rows.append({'qid': i, 'doc': q['doc_id'], 'q': q['question'], 'ref': q['answer'], 'answer': a.get('answer'),
                 'gold': sorted(gold), 'read_hit': bool(gold & read), 'cited_hit': bool(gold & set(a.get('evidence_pages', []))),
                 'pages_read': len(read), 'calls': calls, 'auto_correct': auto, 'answered': i in ans})
n = len(rows); done = [r for r in rows if r['answered']]
print(json.dumps({'n': n, 'answered': len(done),
  'read_hit': round(sum(r['read_hit'] for r in done) / max(len(done), 1), 3),
  'cited_hit': round(sum(r['cited_hit'] for r in done) / max(len(done), 1), 3),
  'auto_correct': round(sum(r['auto_correct'] for r in done) / max(len(done), 1), 3),
  'avg_pages_read': round(sum(r['pages_read'] for r in done) / max(len(done), 1), 2),
  'avg_calls': round(sum(r['calls'] for r in done) / max(len(done), 1), 2)}))
json.dump(rows, open('nav_scores.json', 'w'), indent=1)
