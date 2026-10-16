#!/usr/bin/env python3
"""Ledger promoter: beta -> stable plus planned/in_progress -> beta.

History is {feature_id: [green_sha, ...]}. The beta -> stable rule is
conservative (requires numeric progress_pct >= 100, no pending steps, no
open honesty notes or open_gaps). The planned/in_progress -> beta rule
requires the same evidence, test, integration and streak checks, but
progress_pct < 100, pending steps, honesty notes and open_gaps do NOT block
(beta means 'built, tested, incomplete'); only malformed steps/open_gaps
block, and incompleteness is listed in the report. JSON report is stdout;
Markdown is stderr. Dry runs never modify either input.
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


# Promotable statuses and the transition each one proposes.
PROMOTABLE_TRANSITIONS = {'beta': 'beta->stable', 'planned': 'planned->beta',
                          'in_progress': 'in_progress->beta'}


def beta_completeness_split(feature):
    """Split completeness reasons into (blocking, informational).

    The planned/in_progress -> beta lane only blocks on malformed ledger
    data; numeric progress < 100, pending steps, honesty notes and open gaps
    mean 'built, tested, incomplete' and are reported, never blocking.
    """
    blocking, informational = [], []
    for reason in completeness_reasons(feature):
        if 'malformed' in reason or 'not numeric' in reason or 'not finite' in reason:
            blocking.append(reason)
        else:
            informational.append(reason)
    return blocking, informational


def resolvable_target(test, root, targets):
    """A declared test naming an integration target file that exists on disk
    (or that passed as an integration target in this evidence) counts as
    resolvable even when its fn is not in the Rust unit-test index."""
    match = re.fullmatch(r'tests/([\w-]+)\.rs(?:::[\w:]+)?', test)
    return bool(match) and (
        (root / 'tests' / (match[1] + '.rs')).exists() or match[1] in targets)


def integration_linked(declared, result, targets):
    attributed = result.get('integration_targets_by_test')
    if isinstance(attributed, dict):
        return any(
            test in result.get('passed', [])
            and bool(set(attributed.get(test, [])) & targets)
            for test in declared
        )
    # Older evidence must explicitly bind a passed declaration to a target.
    return any(
        (match := re.fullmatch(r'tests/([\w-]+)\.rs::[\w:]+', test))
        and test in result.get('passed', []) and match[1] in targets
        for test in declared
    )


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
    targets = set(evidence.get('integration_targets_passed', []))
    results = {}
    for fid, feature in sorted(document['features'].items()):
        status = feature.get('status')
        transition = PROMOTABLE_TRANSITIONS.get(status)
        if transition is None:
            results[fid] = {'eligible': False,
                            'reasons': ['status is not beta, planned, or in_progress'],
                            'transition': None, 'informational': []}
            continue
        declared = feature.get('tests', [])
        result = evidence.get('per_feature_test_results', {}).get(fid, {})
        missing_list = []
        if not declared or not isinstance(declared, list):
            missing_list.append('no declared test list')
            declared = []
        if status == 'beta':
            reasons = list(common)
            reasons.extend(completeness_reasons(feature))
            reasons.extend(missing_list)
        else:
            blocking, informational = beta_completeness_split(feature)
            reasons = list(common) + blocking + missing_list
            informational = list(informational)
        if set(result.get('declared', [])) != set(declared):
            reasons.append('declared test evidence differs from ledger')
        if status != 'beta':
            for test in declared:
                if not (any(linked(test, path, name) for path, name, _ in indexed)
                        or resolvable_target(test, root, targets)):
                    reasons.append(f'declared test missing from index: {test}')
        if not set(declared) <= set(result.get('passed', [])) or result.get('failed'):
            reasons.append('declared tests failed or unresolved')
        if not integration_linked(declared, result, targets):
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
        results[fid] = {'eligible': not reasons, 'reasons': reasons,
                        'green_streak': count, 'transition': transition,
                        'informational': informational if status != 'beta' else []}
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
            if results[fid].get('transition') == 'beta->stable':
                document['features'][fid].update(
                    status='stable', passes=True, progress_pct=100, last_tested=now[:10],
                    ledger=dict(promoted_by='post-merge-ci', full_verifier_run=evidence['run_url'],
                                sha=evidence['sha'], promoted_at=now))
            else:
                # planned/in_progress -> beta edits status only (plus a dated
                # note); progress_pct and every other field are left untouched.
                old = document['features'][fid].get('status')
                note = (f"promoted {old}->beta by ledger promoter "
                        f"on {evidence['sha']} ({now[:10]})")
                prior = document['features'][fid].get('notes', '')
                document['features'][fid].update(
                    status='beta',
                    notes=(str(prior) + '\n' if prior else '') + note)
        write(args.features, document)
    promotions = {}
    for fid in eligible:
        promotions.setdefault(results[fid].get('transition'), []).append(fid)
    promotions = {key: sorted(fids) for key, fids in sorted(promotions.items())}
    report = dict(sha=evidence.get('sha'), dry_run=not args.apply,
                  promotions=promotions, features=results)
    print(json.dumps(report, indent=2, sort_keys=True))
    print('# Ledger promotion\n', file=sys.stderr)
    for transition in ('beta->stable', 'planned->beta', 'in_progress->beta'):
        members = sorted(fid for fid in eligible
                         if results[fid].get('transition') == transition)
        print(f'## {transition} ({len(members)})', file=sys.stderr)
        for fid in members:
            if transition == 'beta->stable':
                detail = 'eligible beta → stable'
            else:
                detail = f"eligible {results[fid]['transition']}"
                info = results[fid].get('informational', [])
                if info:
                    detail += '; beta-incomplete: ' + '; '.join(info)
            print(f'- {fid}: {detail}', file=sys.stderr)
    print('## not promoted', file=sys.stderr)
    for fid, result in results.items():
        if result['eligible']:
            continue
        detail = '; '.join(result['reasons'])
        info = result.get('informational', [])
        if info:
            suffix = 'beta-incomplete: ' + '; '.join(info)
            detail = f'{detail}; {suffix}' if detail else suffix
        print(f'- {fid}: {detail}', file=sys.stderr)


if __name__ == '__main__':
    main()
