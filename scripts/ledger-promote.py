#!/usr/bin/env python3
"""Conservative beta promoter. History is {feature_id: [green_sha, ...]}.

Evidence additionally carries main_shas (newest first, including sha) to prove
adjacency, and optional ledger_sha/e2e_sha (default sha). Missing evidence blocks.
JSON report is stdout; Markdown is stderr. Dry runs never modify either input.
"""
import argparse
import datetime as dt
import json
import math
from pathlib import Path
import re
import sys


# Word markers avoid substring matches; bracketed markers match even when adjacent.
# Fail closed: even historical or negated gap wording requires ledger cleanup.
# Trade-offs: Spanish "todo" ("all") and legitimate security wording such as
# "never writes plaintext" also block promotion until the notes are reworded.
STEP_PENDING_MARKERS = (
    r'pending', r'pendientes?', r'todos?', r'tbd', r'wip', r'not\s+built',
    r'unbuilt', r'not\s+implemented', r'no\s+implementado', r'deferred',
    r'stubs?', r'stubbed', r'unimplemented', r'placeholders?', r'not\s+yet',
    r'no\s+route\s+wired\s+yet',
)
OPEN_GAP_MARKERS = (
    r'downgrade[ds]?', r'revertido', r'reverted', r'honest\s+\d+%',
    r'still\s+plaintext', r'plaintext', r'stubs?', r'never\s+been\s+exercised',
    r'not\s+yet', r'pending', r'pendientes?', r'todos?', r'0\s+peers',
    r'stubbed', r'unimplemented', r'placeholders?',
    r'zero\s+code', r'dead\s+code',
) + STEP_PENDING_MARKERS  # every step marker also counts as an open gap in notes
STEP_PENDING_PATTERN = re.compile(
    r'(?<!\w)(?:' + '|'.join(STEP_PENDING_MARKERS)
    + r')(?!\w)|\[\s*\]|\(planned\)', re.I)
OPEN_GAP_PATTERN = re.compile(
    r'(?<!\w)(?:' + '|'.join(OPEN_GAP_MARKERS) + r')(?!\w)', re.I)


def completeness_reasons(feature):
    reasons = []
    progress = feature.get('progress_pct')
    if isinstance(progress, bool) or not isinstance(progress, (int, float)):
        reasons.append(f'progress_pct {progress!r} not numeric')
    elif not math.isfinite(progress):
        reasons.append(f'progress_pct {progress!r} not finite')
    elif progress < 100:
        reasons.append(f'progress_pct {progress} < 100')
    steps = feature.get('steps', [])
    if not isinstance(steps, list):
        reasons.append('steps malformed: expected list')
        steps = []
    for step in steps:
        text = json.dumps(step, ensure_ascii=False) if isinstance(step, dict) else str(step)
        unfinished = False
        if isinstance(step, dict):
            states = [step[key] for key in ('status', 'state') if key in step]
            unfinished = not states or any(
                str(state).strip().lower() not in ('done', 'complete', 'completed', 'implemented')
                for state in states)
        if unfinished or STEP_PENDING_PATTERN.search(text):
            reasons.append(f'step pending: {text[:60]}')
    notes = str(feature.get('notes', ''))
    markers = {}
    for match in OPEN_GAP_PATTERN.finditer(notes):
        markers.setdefault(' '.join(match.group().lower().split()), match.group())
    if markers:
        reasons.append('open honesty note: ' + ', '.join(markers.values()))
    gaps = feature.get('open_gaps', [])
    if not isinstance(gaps, list):
        reasons.append('open_gaps malformed: expected list')
    elif gaps:
        reasons.append(f'open_gaps: {len(gaps)}')
    # honesty_reviewed is provenance only; it never overrides an open gap.
    return reasons


def load(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def rust_tests(root):
    found = []
    # Attributes must belong to this function, rather than a previous test.
    attribute = r'#\[(?:[^\]\"]|\"(?:\\.|[^\"\\])*\")*\]'
    pattern = re.compile(r'((?:\s*' + attribute + r'\s*)+)(?:pub\s+)?(?:async\s+)?fn\s+(\w+)', re.S)
    paths = (p for directory in ('tests', 'src', 'crates', 'code-graph')
             for p in (root / directory).rglob('*.rs'))
    for path in sorted(paths):
        source = re.sub(r'(?m)^\s*//[^\n]*|/\*.*?\*/', '', path.read_text(), flags=re.S)
        for attrs, name in pattern.findall(source):
            if re.search(r'#\[(?:\w+::)*test(?:\s|\]|\()', attrs):
                found.append((path.relative_to(root).as_posix(), name,
                              bool(re.search(r'ignore\s*=\s*"BUG', attrs))))
    return found


def linked(test, path, name):
    return test in (path, Path(path).stem) or test.split('::')[-1] == name


def evaluate(document, evidence, history, root, streak):
    sha = evidence.get('sha')
    chain = evidence.get('main_shas', [sha])
    common = []
    if not sha or evidence.get('ledger_sha', sha) != sha or evidence.get('e2e_sha', sha) != sha:
        common.append('ledger/E2E SHA mismatch or missing SHA')
    if any(evidence.get(k) != 'success' for k in ('ledger_run_conclusion', 'e2e_run_conclusion')):
        common.append('ledger and E2E must both succeed')
    if not evidence.get('run_url'):
        common.append('missing full verifier run URL')
    if not chain or chain[0] != sha or len(set(chain)) != len(chain):
        common.append('invalid consecutive main SHA sequence')
    indexed = rust_tests(root)
    results = {}
    for fid, feature in sorted(document['features'].items()):
        reasons = list(common)
        if feature.get('status') != 'beta':
            results[fid] = {'eligible': False, 'reasons': ['status is not beta']}
            continue
        reasons.extend(completeness_reasons(feature))
        declared = feature.get('tests', [])
        result = evidence.get('per_feature_test_results', {}).get(fid, {})
        if not declared or not isinstance(declared, list):
            reasons.append('no declared test list')
            declared = []
        if set(result.get('declared', [])) != set(declared):
            reasons.append('declared test evidence differs from ledger')
        if not set(declared) <= set(result.get('passed', [])) or result.get('failed'):
            reasons.append('declared tests failed or unresolved')
        targets = set(evidence.get('integration_targets_passed', []))
        attributed = result.get('integration_targets_by_test')
        if isinstance(attributed, dict):
            integration = any(
                test in result.get('passed', [])
                and bool(set(attributed.get(test, [])) & targets)
                for test in declared
            )
        else:
            # Older evidence must explicitly bind a passed declaration to a target.
            integration = any(
                (match := re.fullmatch(r'tests/([\w-]+)\.rs::[\w:]+', test))
                and test in result.get('passed', []) and match[1] in targets
                for test in declared
            )
        if not integration:
            reasons.append('no linked integration/E2E evidence')
        if any(bug and linked(test, path, name)
               for test in declared for path, name, bug in indexed):
            reasons.append('linked test is BUG-ignored')
        green = set(history.get(fid, [])) | {sha}
        count = 0
        for commit in chain:
            if commit not in green:
                break
            count += 1
        if count < streak:
            reasons.append(f'green streak {count} < {streak}')
        results[fid] = {'eligible': not reasons, 'reasons': reasons, 'green_streak': count}
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('features', 'evidence', 'history'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--streak', type=int, default=3)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--dry-run', action='store_true', default=True)
    mode.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    if args.streak < 1:
        parser.error('--streak must be positive')
    document, evidence, history = map(load, (args.features, args.evidence, args.history))
    root = args.features.resolve().parent
    if root.name == '.gitcore':
        root = root.parent
    results = evaluate(document, evidence, history, root, args.streak)
    eligible = [fid for fid, result in results.items() if result['eligible']]
    if args.apply and eligible:
        now = evidence.get('verified_at') or dt.datetime.now(dt.timezone.utc).isoformat()
        for fid in eligible:
            document['features'][fid].update(
                status='stable', passes=True, progress_pct=100, last_tested=now[:10],
                ledger=dict(promoted_by='post-merge-ci', full_verifier_run=evidence['run_url'],
                            sha=evidence['sha'], promoted_at=now))
        write(args.features, document)
    report = dict(sha=evidence.get('sha'), dry_run=not args.apply, features=results)
    print(json.dumps(report, indent=2, sort_keys=True))
    print('# Ledger promotion\n', file=sys.stderr)
    for fid, result in results.items():
        detail = '; '.join(result['reasons']) or 'eligible beta → stable'
        print(f'- {fid}: {detail}', file=sys.stderr)


if __name__ == '__main__':
    main()
