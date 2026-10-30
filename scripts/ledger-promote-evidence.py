#!/usr/bin/env python3
"""Collect trusted push/main run evidence; never execute ledger tests.

Reconstruct history from consecutive completed first-parent commits and their artifacts,
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
import time
import zipfile

spec = importlib.util.spec_from_file_location('promoter', Path(__file__).with_name('ledger-promote.py'))
promoter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(promoter)

COMMIT_WINDOW = 30
STREAK = 3


def gh(endpoint, binary=False):
    attempts = 4
    for attempt in range(attempts):
        try:
            data = subprocess.check_output(['gh', 'api', endpoint], stderr=subprocess.PIPE)
            return data if binary else json.loads(data)
        except subprocess.CalledProcessError as e:
            if attempt == attempts - 1:
                raise
            stderr = e.stderr.decode('utf-8', errors='replace') if e.stderr else ''
            if re.search(r'HTTP (?:5[0-9]{2}|429)(?!\d)|timeout|connection reset', stderr, re.IGNORECASE):
                time.sleep(2 ** (attempt + 1))
            else:
                raise


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
    is_recognized = '(mode: full,' in log and bool(re.search(r'(?m)^\s*✅ all declared tests passed|^\s*(?:Nextest|cargo test) summary|^\s*❌ declared tests failed \(.*?\) — failing pipeline', log))
    if '(mode: full,' in log:
        for match in re.finditer(r'(?m)^\s*▶ ([^\n]+)\n(.*?)(?=^\s*▶ |^\s*✅ all declared tests passed|^\s*(?:Nextest|cargo test) summary|^\s*❌ declared tests failed \(.*?\) — failing pipeline|\Z)', log, re.S):
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
            elif '❌ FAILED:' in block:
                if flagged:
                    candidate_failed.add(test)
                    candidate.add(test)
                else:
                    failed.add(test)

    parsed_tests = passed | failed | candidate_passed | candidate_failed
    evidence_complete = False
    if is_recognized:
        all_declared = set()
        for f in document['features'].values():
            if f.get('status') == 'deferred':
                continue
            decl = f.get('tests', [])
            if isinstance(decl, list):
                for t in decl:
                    if isinstance(t, str):
                        all_declared.add(t)
        # Also, the original script checked '✅ all declared tests passed'
        if all_declared <= parsed_tests:
            evidence_complete = True
    results = {}
    for fid, f in document['features'].items():
        declared = f.get('tests', [])
        declared = declared if isinstance(declared, list) else []
        is_valid_list = bool(declared) and all(isinstance(t, str) and t for t in declared)
        feat_evidence_complete = is_recognized and is_valid_list and all(t in parsed_tests for t in declared)
        valid_declared = [t for t in declared if isinstance(t, str)]
        results[fid] = dict(declared=f.get('tests', []),
                            evidence_complete=feat_evidence_complete,
                            passed=sorted(set(valid_declared) & (passed - failed)),
                            integration_targets_by_test={test: targets_by_test.get(test, [])
                                                         for test in valid_declared
                                                         if test in passed - failed},
                            failed=sorted(set(valid_declared) & failed),
                            candidate_passed=sorted(set(valid_declared) & candidate_passed),
                            candidate_failed=sorted(set(valid_declared) & candidate_failed),
                            candidate_integration_targets_by_test={
                                test: candidate_targets_by_test.get(test, [])
                                for test in valid_declared
                                if test in candidate_passed},
                            candidate_tests=sorted(set(valid_declared) & candidate))
    return results, sorted(targets), evidence_complete


def collect(sha, document, repo):
    evidence = dict(sha=sha, per_feature_test_results={}, integration_targets_passed=[])
    completed = []
    for workflow, key in [('ci.yml', 'ledger'), ('e2e-segmented.yml', 'e2e')]:
        runs = gh(f'{repo}/actions/workflows/{workflow}/runs?branch=main&head_sha={sha}&per_page=100')['workflow_runs']
        runs = [r for r in runs if r['head_sha'] == sha and r['head_branch'] == 'main'
                and r['event'] in ('push', 'workflow_dispatch')]
        if not runs:
            continue
        run = max(runs, key=lambda r: (r['run_number'], r['run_attempt']))
        evidence[key + '_sha'] = run['head_sha']
        if run['status'] != 'completed':
            continue
        completed.append(key)
        if key == 'e2e':
            evidence['e2e_run_conclusion'] = run['conclusion']
            continue
        jobs = gh(f"{repo}/actions/runs/{run['id']}/jobs?per_page=100")['jobs']
        job = next((j for j in jobs if j['name'] == 'Full Ledger Verification'), {})
        evidence.update(ledger_run_conclusion=(job.get('conclusion')
                                              if run.get('conclusion') == 'success'
                                              else run.get('conclusion')), run_url=run['html_url'],
                        verified_at=run['updated_at'])
        artifacts = gh(f"{repo}/actions/runs/{run['id']}/artifacts?per_page=100")['artifacts']
        artifact = next((a for a in artifacts if a['name'] == 'ledger-logs' and not a['expired']), None)
        if artifact and artifact['size_in_bytes'] < 10_000_000:
            archive = gh(f"{repo}/actions/artifacts/{artifact['id']}/zip", binary=True)
            with zipfile.ZipFile(io.BytesIO(archive)) as z:
                if z.getinfo('ledger.log').file_size < 10_000_000:
                    results, targets, evidence_complete = parse_log(z.read('ledger.log').decode(), document)
                    evidence.update(per_feature_test_results=results, integration_targets_passed=targets, evidence_complete=evidence_complete)
    evidence['required_runs_completed'] = len(completed) == 2
    return evidence


def gather(document, repo, root):
    candidates = subprocess.check_output(
        ['git', 'rev-list', '--first-parent', f'-{COMMIT_WINDOW}', 'HEAD'],
        text=True).splitlines()
    evidence, history, chain, skipped = None, {}, [], []
    for sha in candidates:
        # Bind artifact results to the declarations actually verified at this SHA.
        # evaluate() compares them with the current ledger before granting a vote.
        snapshot = json.loads(subprocess.check_output(
            ['git', 'show', f'{sha}:.gitcore/features.json'], text=True))
        record = collect(sha, snapshot, repo)
        if not record['required_runs_completed']:
            skipped.append(sha)
            continue
        chain.append(sha)
        record['main_shas'] = [sha]
        for fid, result in promoter.evaluate(document, record, {}, root, 1).items():
            if result['eligible']:
                history.setdefault(fid, []).append(sha)
        if evidence is None:
            evidence = record
        if len(chain) == STREAK:
            break
    if evidence is None:
        evidence = dict(sha=None, per_feature_test_results={}, integration_targets_passed=[])
    evidence.update(main_shas=chain, main_head=candidates[0] if candidates else None,
                    skipped_pending_shas=skipped)
    return evidence, history


def main():
    document = promoter.load('.gitcore/features.json')
    evidence, history = gather(document, 'repos/' + os.environ['GITHUB_REPOSITORY'], Path.cwd())
    promoter.write('/tmp/ledger-evidence.json', evidence)
    promoter.write('/tmp/ledger-history.json', history)


if __name__ == '__main__':
    main()
