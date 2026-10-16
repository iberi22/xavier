#!/usr/bin/env python3
"""Collect trusted push/main run evidence; never execute ledger tests.

Reconstruct history from consecutive first-parent commits and their artifacts,
so reruns cannot add votes and absent/expired artifacts break the green streak.

The verify pipeline runs `planned` features in a non-gating candidate lane and
marks those log lines `[candidate]`; parse_log strips the marker for test-name
matching and records candidate outcomes ONLY in candidate_passed /
candidate_failed (plus candidate attribution). Gating passed/failed/targets
never contain candidate results, so candidate failures cannot pollute gating
evidence for beta -> stable.
"""
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile

spec = importlib.util.spec_from_file_location('promoter', Path(__file__).with_name('ledger-promote.py'))
promoter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(promoter)


def gh(endpoint, binary=False):
    data = subprocess.check_output(['gh', 'api', endpoint])
    return data if binary else json.loads(data)


CANDIDATE_MARKER = re.compile(r'\s*\[(?:candidate|planned[-\s]?beta)\]\s*$|\s*\(candidate\)\s*$',
                              re.IGNORECASE)


def parse_log(log, document):
    # Gating sets contain ONLY non-candidate lines. Candidate lines go ONLY
    # to candidate_* sets, never merged into gating passed/failed/targets.
    passed, failed, targets = set(), set(), set()
    candidate_passed, candidate_failed = set(), set()
    targets_by_test = {}
    candidate_targets_by_test = {}
    candidate = set()
    if '(mode: full,' in log and '✅ all declared tests passed' in log:
        for match in re.finditer(r'(?m)^\s*▶ ([^\n]+)\n(.*?)(?=^\s*▶ |\Z)', log, re.S):
            raw, block = match.groups()
            flagged = bool(CANDIDATE_MARKER.search(raw))
            test = CANDIDATE_MARKER.sub('', raw).strip()
            successes = re.findall(r'✅ ([^\n]+) \(passed=([1-9][0-9]*)\)', block)
            if successes and '❌ FAILED:' not in block:
                test_targets = set()
                for label, _ in successes:
                    target = re.search(r'--test ([\w-]+)(?=\s|$)', label)
                    if target:
                        test_targets.add(target[1])
                if flagged:
                    candidate_passed.add(test)
                    candidate_targets_by_test[test] = sorted(test_targets)
                    candidate.add(test)
                else:
                    passed.add(test)
                    targets_by_test[test] = sorted(test_targets)
                    targets.update(test_targets)
            else:
                if flagged:
                    candidate_failed.add(test)
                    candidate.add(test)
                else:
                    failed.add(test)
    results = {}
    for fid, f in document['features'].items():
        declared = f.get('tests', [])
        declared = declared if isinstance(declared, list) else []
        results[fid] = dict(declared=f.get('tests', []),
                            passed=sorted(set(declared) & (passed - failed)),
                            integration_targets_by_test={test: targets_by_test.get(test, [])
                                                         for test in declared
                                                         if test in passed - failed},
                            failed=sorted(set(declared) & failed),
                            candidate_passed=sorted(set(declared) & candidate_passed),
                            candidate_failed=sorted(set(declared) & candidate_failed),
                            candidate_integration_targets_by_test={
                                test: candidate_targets_by_test.get(test, [])
                                for test in declared
                                if test in candidate_passed},
                            candidate_tests=sorted(set(declared) & candidate))
    return results, sorted(targets)


def collect(sha, document, repo):
    evidence = dict(sha=sha, per_feature_test_results={}, integration_targets_passed=[])
    for workflow, key in [('ci.yml', 'ledger'), ('e2e-segmented.yml', 'e2e')]:
        runs = gh(f'{repo}/actions/workflows/{workflow}/runs?branch=main&event=push&head_sha={sha}&per_page=100')['workflow_runs']
        runs = [r for r in runs if r['head_sha'] == sha and r['head_branch'] == 'main'
                and r['event'] == 'push']
        if not runs:
            continue
        run = max(runs, key=lambda r: (r['run_number'], r['run_attempt']))
        evidence[key + '_sha'] = run['head_sha']
        if run['status'] != 'completed':
            continue
        if key == 'e2e':
            evidence['e2e_run_conclusion'] = run['conclusion']
            continue
        jobs = gh(f"{repo}/actions/runs/{run['id']}/jobs?per_page=100")['jobs']
        job = next((j for j in jobs if j['name'] == 'Full Ledger Verification'), {})
        evidence.update(ledger_run_conclusion=job.get('conclusion'), run_url=run['html_url'],
                        verified_at=run['updated_at'])
        artifacts = gh(f"{repo}/actions/runs/{run['id']}/artifacts?per_page=100")['artifacts']
        artifact = next((a for a in artifacts if a['name'] == 'ledger-logs' and not a['expired']), None)
        if artifact and artifact['size_in_bytes'] < 10_000_000:
            archive = gh(f"{repo}/actions/artifacts/{artifact['id']}/zip", binary=True)
            with zipfile.ZipFile(io.BytesIO(archive)) as z:
                if z.getinfo('ledger.log').file_size < 10_000_000:
                    results, targets = parse_log(z.read('ledger.log').decode(), document)
                    evidence.update(per_feature_test_results=results, integration_targets_passed=targets)
    return evidence


def main():
    root = Path.cwd()
    document = promoter.load('.gitcore/features.json')
    chain = subprocess.check_output(['git', 'rev-list', '--first-parent', '-3', 'HEAD'], text=True).splitlines()
    evidence, history = None, {}
    for sha in chain:
        record = collect(sha, document, 'repos/' + os.environ['GITHUB_REPOSITORY'])
        record['main_shas'] = [sha]
        for fid, result in promoter.evaluate(document, record, {}, root, 1).items():
            if result['eligible']:
                history.setdefault(fid, []).append(sha)
        if evidence is None:
            evidence = record
    evidence['main_shas'] = chain
    promoter.write('/tmp/ledger-evidence.json', evidence)
    promoter.write('/tmp/ledger-history.json', history)


if __name__ == '__main__':
    main()
