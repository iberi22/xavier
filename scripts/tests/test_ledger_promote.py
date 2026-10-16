import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
FIXTURES = Path(__file__).with_name('fixtures') / 'ledger_promote'
spec = importlib.util.spec_from_file_location('collector', SCRIPTS / 'ledger-promote-evidence.py')
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


class PromoterTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / '.gitcore').mkdir()
        (self.root / 'tests').mkdir()
        self.rust = self.root / 'tests' / 'integration.rs'
        self.rust.write_text('#[test]\nfn test_roundtrip() {}\n')
        self.features = json.loads((FIXTURES / 'features.json').read_text())
        self.evidence = json.loads((FIXTURES / 'evidence.json').read_text())
        self.history = json.loads((FIXTURES / 'history.json').read_text())
        self.path = self.root / '.gitcore' / 'features.json'

    def run_promoter(self, apply=False, initialize=True):
        if initialize:
            self.path.write_text(json.dumps(self.features))
        evidence, history = self.root / 'evidence.json', self.root / 'history.json'
        evidence.write_text(json.dumps(self.evidence))
        history.write_text(json.dumps(self.history))
        before = {p: p.read_bytes() for p in (self.path, evidence, history)}
        result = subprocess.run([sys.executable, str(SCRIPTS / 'ledger-promote.py'),
                                 '--features', str(self.path), '--evidence', str(evidence),
                                 '--history', str(history)] + (['--apply'] if apply else []),
                                capture_output=True, text=True, check=True)
        self.assertIn('# Ledger promotion', result.stderr)
        if not apply:
            self.assertEqual(before, {p: p.read_bytes() for p in before})
        feature = json.loads(result.stdout)['features']['beta']
        for reason in feature['reasons']:
            self.assertIn(reason, result.stderr)
        return feature

    def blocked(self, reason):
        result = self.run_promoter(apply=True)
        self.assertFalse(result['eligible'])
        self.assertIn(reason, '; '.join(result['reasons']))
        self.assertEqual(json.dumps(self.features), self.path.read_text())

    def test_promotes_only_eligible_beta_and_is_idempotent(self):
        self.assertTrue(self.run_promoter(apply=True)['eligible'])
        data = json.loads(self.path.read_text())
        beta = data['features']['beta']
        self.assertEqual(('stable', True, 100), (beta['status'], beta['passes'], beta['progress_pct']))
        self.assertEqual('keep me', beta['notes'])
        self.assertEqual('post-merge-ci', beta['ledger']['promoted_by'])
        self.assertEqual(self.evidence['run_url'], beta['ledger']['full_verifier_run'])
        self.assertEqual('c', beta['ledger']['sha'])
        self.assertEqual('2026-10-15', beta['last_tested'])
        for fid in ('planned', 'stable', 'in_progress'):
            self.assertEqual(self.features['features'][fid], data['features'][fid])
        original = self.path.read_bytes()
        self.assertFalse(self.run_promoter(apply=True, initialize=False)['eligible'])
        self.assertEqual(original, self.path.read_bytes())

    def test_default_dry_run_writes_nothing(self):
        self.assertTrue(self.run_promoter()['eligible'])

    def test_progress_90_blocks(self):
        self.features['features']['beta']['progress_pct'] = 90
        self.blocked('progress_pct 90 < 100')

    def test_progress_missing_blocks(self):
        del self.features['features']['beta']['progress_pct']
        self.blocked('progress_pct None not numeric')

    def test_invalid_progress_blocks(self):
        for progress, reason in (
                ('100', "progress_pct '100' not numeric"),
                (None, 'progress_pct None not numeric'),
                (True, 'progress_pct True not numeric'),
                (float('nan'), 'progress_pct nan not finite'),
                (float('inf'), 'progress_pct inf not finite')):
            with self.subTest(progress=progress):
                self.features['features']['beta']['progress_pct'] = progress
                self.blocked(reason)

    def test_malformed_steps_block(self):
        for steps in ('PENDING', {'transport': 'pending'}, None, False, 100, ''):
            with self.subTest(steps=steps):
                self.features['features']['beta']['steps'] = steps
                self.blocked('steps malformed')

    def test_pending_step_blocks(self):
        self.features['features']['beta']['steps'] = ['PENDING: build transport']
        self.blocked('step pending: PENDING: build transport')

    def test_dict_todo_step_blocks(self):
        self.features['features']['beta']['steps'] = [dict(name='transport', status='todo')]
        self.blocked('step pending: ')

    def test_step_markers_and_dict_states_block(self):
        steps = ['PENDIENTE', 'TODO', 'TBD', 'WIP', 'not built', 'unbuilt',
                 'not implemented', 'no implementado', '[ ] transport',
                 '(planned)', 'deferred', dict(state='pending'),
                 dict(status='unknown'), dict(status='done', state='todo'),
                 dict(name='transport'), dict(status=None), 'stub', 'not yet',
                 '(no route wired yet)', 'unimplemented!()', 'stubbed', 'placeholder']
        for step in steps:
            with self.subTest(step=step):
                self.features['features']['beta']['steps'] = [step]
                self.blocked('step pending: ')

    def test_adjacent_and_empty_checkbox_step_markers_block(self):
        for step in ('[ ]Build', 'Step(planned)', '[]', '[  ]', '[\t]',
                     'Build[]transport', 'Step(planned)Build'):
            with self.subTest(step=step):
                self.features['features']['beta']['steps'] = [step]
                self.blocked('step pending: ')

    def test_plural_markers_block(self):
        for field, markers in (
                ('steps', ('stubs', 'TODOs', 'pendientes')),
                ('notes', ('stubs', 'TODOs', 'pendientes', 'downgrades'))):
            for marker in markers:
                with self.subTest(field=field, marker=marker):
                    self.features['features']['beta'].update(steps=[], notes='keep me')
                    self.features['features']['beta'][field] = (
                        [marker] if field == 'steps' else marker)
                    self.blocked('step pending:' if field == 'steps' else 'open honesty note:')

    def test_step_reason_is_truncated(self):
        step = 'PENDING ' + 'x' * 100
        self.features['features']['beta']['steps'] = [step]
        result = self.run_promoter()
        self.assertEqual(['step pending: ' + step[:60]], result['reasons'])

    def test_downgraded_notes_block(self):
        self.features['features']['beta']['notes'] = 'Downgraded from stable'
        self.blocked('open honesty note: Downgraded')

    def test_honest_percentage_notes_block(self):
        self.features['features']['beta']['notes'] = '(honest 50%)'
        self.blocked('open honesty note: honest 50%')

    def test_open_gap_note_markers_block(self):
        for marker in ('revertido', 'REVERTED', 'still plaintext', 'plaintext', 'stub',
                       'never been exercised', 'not yet', 'pending', 'pendiente',
                       'TODO', '0 peers', 'zero code', 'dead code',
                       'unimplemented!()', 'stubbed', 'placeholder', 'placeholders',
                       'not implemented', 'no implementado', 'not built', 'unbuilt',
                       'WIP', 'TBD', 'deferred'):
            with self.subTest(marker=marker):
                self.features['features']['beta']['notes'] = marker
                self.blocked('open honesty note: ')

    def test_honesty_notes_are_combined_and_deduplicated(self):
        self.features['features']['beta']['notes'] = 'stub STUB pending Pending stub'
        result = self.run_promoter()
        self.assertFalse(result['eligible'])
        self.assertEqual(['open honesty note: stub, pending'], result['reasons'])

    def test_open_gaps_block(self):
        self.features['features']['beta']['open_gaps'] = ['transport missing']
        self.blocked('open_gaps: 1')

    def test_malformed_open_gaps_block(self):
        for gaps in ('transport missing', {'transport': 'missing'}, None, False, 0, '', {}):
            with self.subTest(gaps=gaps):
                self.features['features']['beta']['open_gaps'] = gaps
                self.blocked('open_gaps malformed')

    def test_honesty_review_never_bypasses_blockers(self):
        feature = self.features['features']['beta']
        feature.update(progress_pct=90, steps=['TODO'], notes='stub', open_gaps=['gap'],
                       honesty_reviewed={'sha': 'c', 'by': 'reviewer'})
        result = self.run_promoter()
        self.assertFalse(result['eligible'])
        for reason in ('progress_pct 90 < 100', 'step pending: TODO',
                       'open honesty note: stub', 'open_gaps: 1'):
            self.assertIn(reason, result['reasons'])

    def test_complete_feature_stays_eligible(self):
        self.features['features']['beta'].update(
            progress_pct=100, steps=['Transport implemented', dict(status='DONE'),
                                     dict(state='complete'), dict(status='implemented'),
                                     dict(status='completed'), dict(state='COMPLETED')],
            notes='Implementation verified with full integration evidence.', open_gaps=[])
        self.assertTrue(self.run_promoter()['eligible'])

    def test_marker_substrings_do_not_block(self):
        self.features['features']['beta'].update(
            steps=['Appending values with todoist and wiperson'],
            notes='stubborn plaintextual pendingness methodology')
        self.assertTrue(self.run_promoter()['eligible'])

    def test_real_wave_29_notes_do_not_block(self):
        ledger = json.loads((SCRIPTS.parent / '.gitcore' / 'features.json').read_text())
        notes = ledger['features']['feat-e2e-observability-contract']['notes']
        self.assertIn('No status hand-promotion: green', notes)
        self.features['features']['beta']['notes'] = notes
        self.assertTrue(self.run_promoter()['eligible'])

    def test_real_incomplete_features_block_even_with_green_evidence(self):
        ledger = json.loads((SCRIPTS.parent / '.gitcore' / 'features.json').read_text())
        for fid in ('feat-encryption-at-rest', 'feat-mesh-network', 'feat-vault-hardening'):
            with self.subTest(feature=fid):
                feature = ledger['features'][fid]
                self.assertEqual('beta', feature['status'])
                self.features['features']['beta'] = feature
                declared = feature['tests']
                self.evidence['per_feature_test_results']['beta'] = dict(
                    declared=declared, passed=declared, failed=[],
                    integration_targets_by_test={test: ['integration'] for test in declared})
                result = self.run_promoter()
                self.assertFalse(result['eligible'])
                self.assertIn(f"progress_pct {feature['progress_pct']} < 100", result['reasons'])
                self.assertTrue(any(reason.startswith('open honesty note:')
                                    for reason in result['reasons']))

    def test_failed_or_missing_test(self):
        for change in ({'passed': []}, {'failed': ['test_roundtrip']}, {'declared': []}):
            with self.subTest(change=change):
                self.setUp()
                self.evidence['per_feature_test_results']['beta'].update(change)
                self.blocked('declared test')

    def test_no_integration(self):
        self.evidence['per_feature_test_results']['beta']['integration_targets_by_test'] = {}
        self.blocked('no linked integration')

    def test_explicit_target_evidence(self):
        self.rust.unlink()
        test = 'tests/integration.rs::test_roundtrip'
        self.features['features']['beta']['tests'] = [test]
        self.evidence['per_feature_test_results']['beta'] = dict(
            declared=[test], passed=[test], failed=[])
        self.assertTrue(self.run_promoter()['eligible'])
        self.evidence['integration_targets_passed'] = ['other']
        self.blocked('no linked integration')

    def test_empty_and_missing_tests(self):
        for missing in (False, True):
            with self.subTest(missing=missing):
                self.features['features']['beta']['tests'] = []
                if missing:
                    del self.features['features']['beta']['tests']
                self.evidence['per_feature_test_results']['beta'] = dict(
                    declared=[], passed=[], failed=[], integration_targets_by_test={})
                self.blocked('no declared test list')

    def test_missing_run_url(self):
        del self.evidence['run_url']
        self.blocked('missing full verifier run URL')

    def test_non_promotable_status_with_complete_evidence(self):
        self.assertTrue(self.run_promoter()['eligible'])
        self.features['features']['beta']['status'] = 'stable'
        self.blocked('status is not beta, planned, or in_progress')

    def test_unit_only_name_collision_is_rejected(self):
        (self.root / 'src').mkdir()
        (self.root / 'src' / 'lib.rs').write_text('#[test]\nfn test_roundtrip() {}')
        # The integration file has the same name, but only --lib passed.
        log = ('==> Xavier verify pipeline (mode: full, root: /repo)\n'
               '  ▶ test_roundtrip\n    ✅ xavier --lib (passed=1)\n'
               '  ▶ unrelated\n    ✅ --test integration (passed=1)\n'
               '  ✅ all declared tests passed\n')
        results, targets = collector.parse_log(log, self.features)
        self.assertEqual(['test_roundtrip'], results['beta']['passed'])
        self.assertEqual({'test_roundtrip': []}, results['beta']['integration_targets_by_test'])
        self.assertEqual(['integration'], targets)
        self.evidence.update(per_feature_test_results=results, integration_targets_passed=targets)
        self.blocked('no linked integration')

    def test_missing_attribution_rejects_bare_name(self):
        del self.evidence['per_feature_test_results']['beta']['integration_targets_by_test']
        self.blocked('no linked integration')

    def test_attribution_requires_successful_target(self):
        self.evidence['integration_targets_passed'] = ['other']
        self.blocked('no linked integration')

    def test_explicit_declaration_cannot_override_unit_attribution(self):
        test = 'tests/integration.rs::test_roundtrip'
        self.features['features']['beta']['tests'] = [test]
        self.evidence['per_feature_test_results']['beta'] = dict(
            declared=[test], passed=[test], failed=[], integration_targets_by_test={test: []})
        self.blocked('no linked integration')

    def test_bug_ignored_function_and_target(self):
        for test in ('test_roundtrip', 'tests/integration.rs', 'integration'):
            with self.subTest(test=test):
                self.rust.write_text('#[tokio::test]\n#[ignore = "BUG: [tracking] broken"]\n// reason\nasync fn test_roundtrip() {}')
                self.features['features']['beta']['tests'] = [test]
                self.evidence['per_feature_test_results']['beta'] = dict(declared=[test], passed=[test], failed=[])
                self.blocked('BUG-ignored')

    def test_streak_gaps_duplicates_and_insufficient_history(self):
        for history in ([], ['a'], ['a', 'unrelated'], ['b', 'b']):
            with self.subTest(history=history):
                self.history['beta'] = history
                self.blocked('green streak')

    def test_wrong_conclusions_or_sha(self):
        for field, value in [('ledger_run_conclusion', 'failure'), ('e2e_run_conclusion', None),
                             ('ledger_sha', 'other'), ('e2e_sha', 'other'), ('sha', '')]:
            with self.subTest(field=field):
                self.evidence = json.loads((FIXTURES / 'evidence.json').read_text())
                self.evidence[field] = value
                self.blocked('SHA' if 'sha' in field else 'both succeed')

    def test_bug_ignored_unit_test_also_blocks(self):
        (self.root / 'src').mkdir()
        (self.root / 'src' / 'lib.rs').write_text(
            '#[test]\n#[ignore = "BUG: duplicate"]\nfn test_roundtrip() {}')
        self.blocked('BUG-ignored')

    def test_unrelated_qualified_unit_name_is_not_integration(self):
        test = 'unit::test_roundtrip'
        self.features['features']['beta']['tests'] = [test]
        self.evidence['per_feature_test_results']['beta'] = dict(declared=[test], passed=[test], failed=[])
        self.blocked('no linked integration')

    def test_invalid_main_sequence(self):
        for chain in (['other', 'b', 'a'], ['c', 'b', 'b'], []):
            with self.subTest(chain=chain):
                self.evidence['main_shas'] = chain
                self.blocked('main SHA sequence')

    def test_collector_rejects_wrong_sha_branch_and_event(self):
        for fields in ({'head_sha': 'other'}, {'head_branch': 'topic'}, {'event': 'pull_request'}):
            run = dict(head_sha='c', head_branch='main', event='push')
            run.update(fields)
            with patch.object(collector, 'gh', return_value={'workflow_runs': [run]}):
                record = collector.collect('c', self.features, 'repos/example/repo')
            self.assertNotIn('ledger_run_conclusion', record)
            self.assertNotIn('e2e_run_conclusion', record)

    def test_log_parser_requires_full_positive_exact_result(self):
        log = '==> Xavier verify pipeline (mode: full, root: /repo)\n  ▶ test_roundtrip\n    ✅ --test integration (passed=2)\n  ✅ all declared tests passed\n'
        results, targets = collector.parse_log(log, self.features)
        self.assertEqual(['test_roundtrip'], results['beta']['passed'])
        self.assertEqual(['integration'], targets)
        self.assertEqual({'test_roundtrip': ['integration']},
                         results['beta']['integration_targets_by_test'])
        for invalid in (log.replace('passed=2', 'passed=0'), log.replace('full,', 'check,'),
                        log.replace('▶ test_roundtrip', '▶ test_other'),
                        log.replace('✅ all declared tests passed', '')):
            self.assertEqual([], collector.parse_log(invalid, self.features)[0]['beta']['passed'])


class PlannedBetaPromotionTests(unittest.TestCase):
    """planned/in_progress -> beta lane (beta -> stable is covered above)."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / '.gitcore').mkdir()
        (self.root / 'tests').mkdir()
        (self.root / 'tests' / 'integration.rs').write_text('#[test]\nfn test_roundtrip() {}\n')
        self.features = json.loads((FIXTURES / 'features.json').read_text())
        self.features['features']['planned'] = {
            'status': 'planned', 'progress_pct': 40,
            'tests': ['test_roundtrip'],
            'steps': ['PENDING: build transport'],
            'notes': 'early prototype, TODO remaining',
        }
        self.evidence = json.loads((FIXTURES / 'evidence.json').read_text())
        self.evidence['per_feature_test_results']['planned'] = dict(
            declared=['test_roundtrip'], passed=['test_roundtrip'], failed=[],
            integration_targets_by_test={'test_roundtrip': ['integration']})
        self.history = json.loads((FIXTURES / 'history.json').read_text())
        self.history['planned'] = ['a', 'b']
        self.path = self.root / '.gitcore' / 'features.json'

    def run_all(self, apply=False):
        self.path.write_text(json.dumps(self.features))
        evidence, history = self.root / 'evidence.json', self.root / 'history.json'
        evidence.write_text(json.dumps(self.evidence))
        history.write_text(json.dumps(self.history))
        before = {p: p.read_bytes() for p in (self.path, evidence, history)}
        result = subprocess.run([sys.executable, str(SCRIPTS / 'ledger-promote.py'),
                                 '--features', str(self.path), '--evidence', str(evidence),
                                 '--history', str(history)] + (['--apply'] if apply else []),
                                capture_output=True, text=True, check=True)
        self.assertIn('# Ledger promotion', result.stderr)
        if not apply:
            self.assertEqual(before, {p: p.read_bytes() for p in before})
        return json.loads(result.stdout), result.stderr

    def test_planned_eligible_promotes_to_beta(self):
        report, err = self.run_all(apply=True)
        planned = report['features']['planned']
        self.assertTrue(planned['eligible'], planned['reasons'])
        self.assertEqual('planned->beta', planned['transition'])
        self.assertIn('progress_pct 40 < 100', ' ; '.join(planned['informational']))
        self.assertTrue(any(reason.startswith('step pending:')
                            for reason in planned['informational']))
        self.assertTrue(any(reason.startswith('open honesty note:')
                            for reason in planned['informational']))
        data = json.loads(self.path.read_text())['features']['planned']
        self.assertEqual('beta', data['status'])
        self.assertEqual(40, data['progress_pct'])
        self.assertNotIn('passes', data)
        self.assertNotIn('ledger', data)
        self.assertIn('promoted planned->beta by ledger promoter on c', data['notes'])
        self.assertEqual(['planned'], report['promotions'].get('planned->beta'))
        self.assertIn('## planned->beta (1)', err)

    def test_planned_dry_run_writes_nothing(self):
        report, _ = self.run_all()
        self.assertTrue(report['features']['planned']['eligible'])

    def test_planned_failing_test_blocked(self):
        self.evidence['per_feature_test_results']['planned'] = dict(
            declared=['test_roundtrip'], passed=[], failed=['test_roundtrip'],
            integration_targets_by_test={})
        report, _ = self.run_all(apply=True)
        planned = report['features']['planned']
        self.assertFalse(planned['eligible'])
        self.assertIn('declared tests failed or unresolved', '; '.join(planned['reasons']))
        self.assertEqual('planned', json.loads(self.path.read_text())['features']['planned']['status'])

    def test_planned_missing_test_blocked(self):
        self.features['features']['planned']['tests'] = ['ghost_test']
        self.evidence['per_feature_test_results']['planned'] = dict(
            declared=[], passed=[], failed=[], integration_targets_by_test={})
        report, _ = self.run_all(apply=True)
        planned = report['features']['planned']
        self.assertFalse(planned['eligible'])
        self.assertIn('declared test missing from index: ghost_test',
                      '; '.join(planned['reasons']))
        self.assertEqual('planned', json.loads(self.path.read_text())['features']['planned']['status'])

    def test_planned_lib_only_blocked(self):
        self.evidence['per_feature_test_results']['planned']['integration_targets_by_test'] = {
            'test_roundtrip': []}
        self.evidence['integration_targets_passed'] = ['other']
        report, _ = self.run_all(apply=True)
        planned = report['features']['planned']
        self.assertFalse(planned['eligible'])
        self.assertIn('no linked integration', '; '.join(planned['reasons']))
        self.assertEqual('planned', json.loads(self.path.read_text())['features']['planned']['status'])

    def test_in_progress_eligible(self):
        self.features['features']['in_progress'] = dict(
            self.features['features']['planned'], status='in_progress')
        self.evidence['per_feature_test_results']['in_progress'] = dict(
            self.evidence['per_feature_test_results']['planned'])
        self.history['in_progress'] = ['a', 'b']
        report, _ = self.run_all(apply=True)
        progressing = report['features']['in_progress']
        self.assertTrue(progressing['eligible'], progressing['reasons'])
        data = json.loads(self.path.read_text())['features']['in_progress']
        self.assertEqual('beta', data['status'])
        self.assertIn('promoted in_progress->beta by ledger promoter on c', data['notes'])
        self.assertEqual(['in_progress'], report['promotions'].get('in_progress->beta'))

    def test_malformed_steps_block_planned(self):
        self.features['features']['planned']['steps'] = 'PENDING'
        report, _ = self.run_all(apply=True)
        planned = report['features']['planned']
        self.assertFalse(planned['eligible'])
        self.assertIn('steps malformed', '; '.join(planned['reasons']))
        self.assertEqual('planned', json.loads(self.path.read_text())['features']['planned']['status'])

    def test_report_groups_promotions_by_transition(self):
        report, err = self.run_all()
        self.assertEqual(['beta'], report['promotions'].get('beta->stable'))
        self.assertEqual(['planned'], report['promotions'].get('planned->beta'))
        for header in ('## beta->stable (1)', '## planned->beta (1)',
                       '## in_progress->beta (0)', '## not promoted'):
            self.assertIn(header, err)

    def test_candidate_marker_is_parsed(self):
        log = ('==> Xavier verify pipeline (mode: full, root: /repo)\n'
               '  ▶ test_roundtrip [candidate]\n    ✅ --test integration (passed=2)\n'
               '  ✅ all declared tests passed\n')
        results, targets = collector.parse_log(log, self.features)
        self.assertEqual(['test_roundtrip'], results['planned']['passed'])
        self.assertEqual(['test_roundtrip'], results['planned']['candidate_tests'])
        self.assertEqual(['integration'], targets)
        plain = log.replace(' [candidate]', '')
        results, _ = collector.parse_log(plain, self.features)
        self.assertEqual([], results['planned']['candidate_tests'])
        self.assertEqual(['test_roundtrip'], results['planned']['passed'])


if __name__ == '__main__':
    unittest.main()
