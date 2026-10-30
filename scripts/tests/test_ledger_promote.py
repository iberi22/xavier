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
        self.evidence['evidence_complete'] = True
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
        results, targets, evidence_complete = collector.parse_log(log, self.features)
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
                self.evidence['evidence_complete'] = True
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


    def test_parse_log_unrecognized_returns_incomplete_flag(self):
        log = "Truncated log... no success marker"
        results, targets, evidence_complete = collector.parse_log(log, self.features)
        self.assertFalse(evidence_complete)

    def test_log_parser_requires_full_positive_exact_result(self):
        log = '==> Xavier verify pipeline (mode: full, root: /repo)\n  ▶ test_roundtrip\n    ✅ --test integration (passed=2)\n  ✅ all declared tests passed\n'
        results, targets, evidence_complete = collector.parse_log(log, self.features)
        self.assertEqual(['test_roundtrip'], results['beta']['passed'])
        self.assertEqual(['integration'], targets)
        self.assertEqual({'test_roundtrip': ['integration']},
                         results['beta']['integration_targets_by_test'])
        for invalid in (log.replace('passed=2', 'passed=0'), log.replace('full,', 'check,'),
                        log.replace('▶ test_roundtrip', '▶ test_other')):
            res, tgt, evc = collector.parse_log(invalid, self.features)
            self.assertEqual([], res['beta']['passed'])

        res, tgt, evc = collector.parse_log(log.replace('full,', 'check,'), self.features)
        self.assertFalse(evc)
        res, tgt, evc = collector.parse_log(log.replace('▶ test_roundtrip', '▶ test_other'), self.features)
        self.assertFalse(evc)

        # Missing footer, but complete coverage means evc = True
        res, tgt, evc = collector.parse_log(log.replace('✅ all declared tests passed', ''), self.features)
        self.assertTrue(evc)


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
        self.evidence['evidence_complete'] = True
        self.evidence['per_feature_test_results']['planned'] = dict(
            declared=['test_roundtrip'], passed=[], failed=[],
            integration_targets_by_test={},
            candidate_passed=['test_roundtrip'], candidate_failed=[],
            candidate_integration_targets_by_test={'test_roundtrip': ['integration']},
            candidate_tests=['test_roundtrip'])
        # The evidence collector always emits a per-feature evidence_complete
        # flag; stable review only runs when it is explicitly True.
        self.evidence['per_feature_test_results']['stable'] = dict(
            declared=[], passed=[], failed=[], integration_targets_by_test={},
            evidence_complete=True)
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
            declared=['test_roundtrip'], passed=[], failed=[],
            integration_targets_by_test={},
            candidate_passed=[], candidate_failed=['test_roundtrip'],
            candidate_integration_targets_by_test={}, candidate_tests=['test_roundtrip'])
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
        self.evidence['per_feature_test_results']['planned'] = dict(
            declared=['test_roundtrip'], passed=[], failed=[],
            integration_targets_by_test={},
            candidate_passed=['test_roundtrip'], candidate_failed=[],
            candidate_integration_targets_by_test={'test_roundtrip': []},
            candidate_tests=['test_roundtrip'])
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

    def test_open_gaps_block_planned(self):
        self.features['features']['planned']['open_gaps'] = ['transport missing']
        report, _ = self.run_all(apply=True)
        planned = report['features']['planned']
        self.assertFalse(planned['eligible'])
        self.assertIn('open_gaps: 1', '; '.join(planned['reasons']))
        self.assertEqual('planned', json.loads(self.path.read_text())['features']['planned']['status'])

    def test_planned_reads_candidate_sets_not_gating(self):
        # Gating sets are empty; only candidate evidence exists -> eligible.
        report, _ = self.run_all()
        planned = report['features']['planned']
        self.assertTrue(planned['eligible'], planned['reasons'])
        self.assertEqual(['test_roundtrip'], planned['candidate_passed'])
        self.assertEqual([], planned['gating_passed'])
        self.assertEqual(['test_roundtrip'], planned['evaluated_passed'])
        self.assertEqual('candidate+gating(shared)', planned['evidence_lane'])
        # Same test name but with no candidate evidence and no shared gating
        # name -> blocked even though gating passed contains other names.
        self.features['features']['planned']['tests'] = ['test_roundtrip']
        self.evidence['per_feature_test_results']['planned'] = dict(
            declared=['test_roundtrip'], passed=[], failed=[],
            integration_targets_by_test={},
            candidate_passed=[], candidate_failed=[],
            candidate_integration_targets_by_test={}, candidate_tests=[])
        # Make the name unshared: beta declares something else.
        self.features['features']['beta']['tests'] = ['other_test']
        self.evidence['per_feature_test_results']['beta'] = dict(
            declared=['other_test'], passed=['test_roundtrip'], failed=[],
            integration_targets_by_test={})
        report, _ = self.run_all()
        self.assertFalse(report['features']['planned']['eligible'])
        self.assertIn('declared tests failed or unresolved',
                      '; '.join(report['features']['planned']['reasons']))

    def test_shared_name_candidate_failure_does_not_block_stable(self):
        # Planned and beta share test_roundtrip; candidate run fails but
        # gating run passes -> beta stays eligible, planned is blocked.
        self.evidence['per_feature_test_results']['beta'] = dict(
            declared=['test_roundtrip'], passed=['test_roundtrip'], failed=[],
            integration_targets_by_test={'test_roundtrip': ['integration']},
            candidate_passed=[], candidate_failed=['test_roundtrip'],
            candidate_integration_targets_by_test={}, candidate_tests=['test_roundtrip'])
        self.evidence['per_feature_test_results']['planned'] = dict(
            declared=['test_roundtrip'], passed=[], failed=[],
            integration_targets_by_test={},
            candidate_passed=[], candidate_failed=['test_roundtrip'],
            candidate_integration_targets_by_test={}, candidate_tests=['test_roundtrip'])
        report, _ = self.run_all()
        self.assertTrue(report['features']['beta']['eligible'],
                        report['features']['beta']['reasons'])
        self.assertEqual('gating', report['features']['beta']['evidence_lane'])
        self.assertFalse(report['features']['planned']['eligible'])

    def test_shared_name_gating_evidence_satisfies_planned(self):
        # Planned test shared with gating: gating evidence alone suffices
        # (candidate lane skips names already in TEST_LIST).
        self.evidence['per_feature_test_results']['planned'] = dict(
            declared=['test_roundtrip'], passed=['test_roundtrip'], failed=[],
            integration_targets_by_test={'test_roundtrip': ['integration']},
            candidate_passed=[], candidate_failed=[],
            candidate_integration_targets_by_test={}, candidate_tests=[])
        report, _ = self.run_all()
        self.assertTrue(report['features']['planned']['eligible'],
                        report['features']['planned']['reasons'])

    def test_candidate_marker_is_parsed(self):
        log = ('==> Xavier verify pipeline (mode: full, root: /repo)\n'
               '  ▶ test_roundtrip [candidate]\n    ✅ --test integration (passed=2)\n'
               '  ✅ all declared tests passed\n')
        results, targets, evidence_complete = collector.parse_log(log, self.features)
        # Candidate results never pollute gating sets.
        self.assertEqual([], results['planned']['passed'])
        self.assertEqual([], results['planned']['failed'])
        self.assertEqual(['test_roundtrip'], results['planned']['candidate_passed'])
        self.assertEqual([], results['planned']['candidate_failed'])
        self.assertEqual(['test_roundtrip'], results['planned']['candidate_tests'])
        self.assertEqual({'test_roundtrip': ['integration']},
                         results['planned']['candidate_integration_targets_by_test'])
        self.assertEqual([], targets)
        plain = log.replace(' [candidate]', '')
        results, targets, evidence_complete = collector.parse_log(plain, self.features)
        self.assertEqual([], results['planned']['candidate_tests'])
        self.assertEqual([], results['planned']['candidate_passed'])
        self.assertEqual(['test_roundtrip'], results['planned']['passed'])
        self.assertEqual(['integration'], targets)

    def test_candidate_failure_does_not_pollute_gating(self):
        log = ('==> Xavier verify pipeline (mode: full, root: /repo)\n'
               '  ▶ test_roundtrip\n    ✅ --test integration (passed=2)\n'
               '  ▶ test_roundtrip [candidate]\n    ❌ FAILED: test_roundtrip\n'
               '  ✅ all declared tests passed\n')
        results, targets, evidence_complete = collector.parse_log(log, self.features)
        self.assertEqual(['test_roundtrip'], results['beta']['passed'])
        self.assertEqual([], results['beta']['failed'])
        self.assertEqual([], results['beta']['candidate_passed'])
        self.assertEqual(['test_roundtrip'], results['beta']['candidate_failed'])
        self.assertEqual(['integration'], targets)





    def test_unrecognized_log_empty_tests_list_no_demotion(self):
        # (a) unrecognized log + feature with empty tests list => no demotion
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.features['features']['stable']['tests'] = []
        self.evidence['evidence_complete'] = False
        self.evidence['per_feature_test_results'] = {'stable': {'evidence_complete': False, 'declared': [], 'passed': [], 'failed': [], 'integration_targets_by_test': {}}}
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertIn('review skipped: evidence collection incomplete', stable['review_reasons'])

    def test_global_incomplete_per_feature_complete_no_demotion(self):
        # (b) global incomplete + per-feature complete => no demotion
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.features['features']['stable']['tests'] = ['test_stable']
        self.evidence['evidence_complete'] = False
        self.evidence['per_feature_test_results'] = {'stable': {'evidence_complete': True, 'declared': ['test_stable'], 'passed': ['test_stable'], 'failed': [], 'integration_targets_by_test': {'test_stable': ['int']}}}
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertTrue(any('review skipped: evidence collection incomplete' in r for r in stable['review_reasons']))

    def test_partial_coverage_skipped_with_reason(self):
        # (c) partial coverage => skipped with reason, no demotion
        # Actually for stable, partial coverage (incomplete evidence) means review skipped.
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.features['features']['stable']['tests'] = ['test_1', 'test_2']
        self.evidence['evidence_complete'] = True
        self.evidence['per_feature_test_results'] = {'stable': {'evidence_complete': False, 'declared': ['test_1', 'test_2'], 'passed': ['test_1'], 'failed': [], 'integration_targets_by_test': {'test_1': ['int']}}}
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertIn('review skipped: incomplete evidence', stable['review_reasons'])

    def test_full_coverage_healthy_stable_no_skip_reason(self):
        # (d) full coverage with per-feature evidence_complete=True on a healthy stable feature => reviewed, stays stable, no skip reason asserted
        self.features['features']['stable']['tests'] = ['test_stable']
        if 'open_gaps' in self.features['features']['stable']:
            del self.features['features']['stable']['open_gaps']
        self.evidence['evidence_complete'] = True
        self.evidence['per_feature_test_results'] = {'stable': {'evidence_complete': True, 'declared': ['test_stable'], 'passed': ['test_stable'], 'failed': [], 'integration_targets_by_test': {'test_stable': ['int']}}}
        self.evidence['integration_targets_passed'] = ['int']
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertEqual([], stable['review_reasons'])

    def test_full_coverage_failed_test_demotion_only_with_flag(self):
        # (e) full coverage with a failed declared test => demotion proposed only with --apply-demotions
        self.features['features']['stable']['tests'] = ['test_stable']
        if 'open_gaps' in self.features['features']['stable']:
            del self.features['features']['stable']['open_gaps']
        self.evidence['evidence_complete'] = True
        self.evidence['per_feature_test_results'] = {'stable': {'evidence_complete': True, 'declared': ['test_stable'], 'passed': [], 'failed': ['test_stable'], 'integration_targets_by_test': {}}}
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertTrue(stable.get('review_flagged'))
        self.assertIn('declared tests failed: test_stable', stable['review_reasons'])
        # apply without apply-demotions
        report_apply, _ = self.run_all(apply=True)
        self.assertEqual([], report_apply.get('demotions_applied', []))
        self.assertEqual('stable', json.loads(self.path.read_text())['features']['stable']['status'])
        # apply with apply-demotions demotes the fully evidenced failing feature
        report_dem = self.run_apply_demotions()
        self.assertIn('stable', report_dem['demotions_applied'])
        self.assertEqual('beta', json.loads(self.path.read_text())['features']['stable']['status'])

    def run_apply_demotions(self):
        self.path.write_text(json.dumps(self.features))
        evidence, history = self.root / 'evidence.json', self.root / 'history.json'
        evidence.write_text(json.dumps(self.evidence))
        history.write_text(json.dumps(self.history))
        result = subprocess.run([sys.executable, str(SCRIPTS / 'ledger-promote.py'),
                                 '--features', str(self.path), '--evidence', str(evidence),
                                 '--history', str(history), '--apply', '--apply-demotions'],
                                capture_output=True, text=True, check=True)
        return json.loads(result.stdout)

    def assert_incomplete_feature_evidence_never_demotes(self):
        # Global evidence is complete and the feature has a demotion trigger
        # (open_gaps + failed declared test), but its own completeness is not
        # proven: review must be skipped, never flagged or demoted.
        self.features['features']['stable']['tests'] = ['test_stable']
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.evidence['evidence_complete'] = True
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertIsNone(stable.get('transition'))
        self.assertEqual(['review skipped: incomplete evidence'], stable['review_reasons'])
        report_dem = self.run_apply_demotions()
        self.assertEqual([], report_dem.get('demotions_applied', []))
        self.assertEqual('stable', json.loads(self.path.read_text())['features']['stable']['status'])

    def test_missing_per_feature_flag_never_demotes(self):
        self.evidence['per_feature_test_results']['stable'] = dict(
            declared=['test_stable'], passed=[], failed=['test_stable'],
            integration_targets_by_test={})
        self.assert_incomplete_feature_evidence_never_demotes()

    def test_null_per_feature_flag_never_demotes(self):
        self.evidence['per_feature_test_results']['stable'] = dict(
            declared=['test_stable'], passed=[], failed=['test_stable'],
            integration_targets_by_test={}, evidence_complete=None)
        self.assert_incomplete_feature_evidence_never_demotes()

    def test_missing_per_feature_result_never_demotes(self):
        del self.evidence['per_feature_test_results']['stable']
        self.assert_incomplete_feature_evidence_never_demotes()


    def test_truncated_final_block_regression(self):
        log = '==> Xavier verify pipeline (mode: full, root: /repo)\n  ▶ test_stable\n  ▶ test_unfinished'
        import importlib.util
        spec = importlib.util.spec_from_file_location('evidence', 'scripts/ledger-promote-evidence.py')
        evidence_mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(evidence_mod)
        self.features['features']['stable']['tests'] = ['test_stable']
        results, _, _ = evidence_mod.parse_log(log, self.features)
        self.assertNotIn('test_stable', results['stable']['passed'])
        self.assertNotIn('test_stable', results['stable']['failed'])
        self.assertNotIn('test_unfinished', results['stable']['passed'])
        self.assertNotIn('test_unfinished', results['stable']['failed'])

    def test_parser_empty_missing_malformed_partial_complete(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('evidence', 'scripts/ledger-promote-evidence.py')
        evidence_mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(evidence_mod)

        doc = {
            'features': {
                'missing': {'status': 'beta'},
                'empty': {'status': 'beta', 'tests': []},
                'malformed_obj': {'status': 'beta', 'tests': [{}]},
                'malformed_empty_str': {'status': 'beta', 'tests': ['']},
                'malformed_list': {'status': 'beta', 'tests': [[]]},
                'partial': {'status': 'beta', 'tests': ['test_1', 'test_2']},
                'complete': {'status': 'beta', 'tests': ['test_1']}
            }
        }
        log = '==> Xavier verify pipeline (mode: full, root: /repo)\n  ▶ test_1\n    ✅ xavier (passed=1)\n  ✅ all declared tests passed\n'
        results, _, evidence_complete = evidence_mod.parse_log(log, doc)
        self.assertFalse(results['missing']['evidence_complete'])
        self.assertFalse(results['empty']['evidence_complete'])
        self.assertFalse(results['malformed_obj']['evidence_complete'])
        self.assertFalse(results['malformed_empty_str']['evidence_complete'])
        self.assertFalse(results['malformed_list']['evidence_complete'])
        self.assertFalse(results['partial']['evidence_complete'])
        self.assertTrue(results['complete']['evidence_complete'])
        self.assertFalse(evidence_complete) # since 'test_2' was missing overall


    def test_stable_incomplete_evidence_collection_skips_review(self):
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.evidence['per_feature_test_results'] = {}
        self.evidence['evidence_complete'] = False
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertTrue(any('review skipped: evidence collection incomplete' in r for r in stable['review_reasons']))

    def test_stable_open_gaps_flagged(self):
        self.features['features']['stable']['open_gaps'] = ['pending']
        report, err = self.run_all()
        stable = report['features']['stable']
        self.assertTrue(stable.get('review_flagged'))
        self.assertIn('open_gaps present', stable['review_reasons'])
        self.assertIn('## stable->beta review', err)


    def test_stable_incomplete_evidence_demotions_enabled(self):
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.evidence['evidence_complete'] = False
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertTrue(any('review skipped: evidence collection incomplete' in r for r in stable['review_reasons']))

    def test_stable_no_linked_evidence_flagged(self):
        self.features['features']['stable']['tests'] = ['test_stable']
        self.evidence['per_feature_test_results']['stable'] = dict(
            declared=['test_stable'], passed=['test_stable'], failed=[],
            integration_targets_by_test={}, evidence_complete=True)
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertTrue(stable.get('review_flagged'))
        self.assertIn('no linked integration/E2E evidence', stable['review_reasons'])

    def test_fully_evidenced_stable_not_flagged(self):
        self.features['features']['stable']['tests'] = ['test_stable']
        self.evidence['per_feature_test_results']['stable'] = dict(
            declared=['test_stable'], passed=['test_stable'], failed=[],
            integration_targets_by_test={'test_stable': ['integration']},
            evidence_complete=True)
        report, err = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertNotIn('test_stable', err)

    def test_common_evidence_failure_flags_nothing(self):
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.evidence['ledger_sha'] = 'mismatch'
        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertFalse(stable.get('review_flagged'))
        self.assertTrue(any('review skipped' in r for r in stable['review_reasons']))


    def test_completed_failure_evidence_reaches_stable_review_from_log(self):
        log = (FIXTURES / 'failed-evidence.log').read_text()
        self.features['features']['stable']['tests'] = ['test_stable']
        self.evidence['ledger_run_conclusion'] = 'failure'

        import importlib.util
        spec = importlib.util.spec_from_file_location('evidence', 'scripts/ledger-promote-evidence.py')
        evidence_mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(evidence_mod)

        results, targets, evidence_complete = evidence_mod.parse_log(log, self.features)
        self.evidence['per_feature_test_results'] = results
        self.evidence['evidence_complete'] = evidence_complete

        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertTrue(stable.get('review_flagged'))
        self.assertIn('declared tests failed: test_stable', stable['review_reasons'])

        # also run the fixture through --apply WITHOUT --apply-demotions and assert every promotion is ineligible, demotions_applied is empty, and the ledger file bytes are unchanged.
        report_apply, _ = self.run_all(apply=True)
        self.assertEqual([], report_apply.get('demotions_applied', []))
        for fid, res in report_apply['features'].items():
            self.assertFalse(res.get('eligible'))

        import json
        with open(self.path) as f:
            data = json.load(f)
        self.assertEqual('stable', data['features']['stable']['status'])

        # exact parsed-result assertions that fail if the parser change is reverted
        self.assertEqual(['test_roundtrip'], results['beta']['passed'])
        self.assertEqual([], results['beta']['failed'])
        self.assertEqual([], results['stable']['passed'])
        self.assertEqual(['test_stable'], results['stable']['failed'])

    def test_completed_failure_evidence_reaches_stable_review(self):
        # A test fails, verifier reports failure, but all tests were parsed
        self.features['features']['stable']['tests'] = ['test_stable']
        self.evidence['ledger_run_conclusion'] = 'failure'
        self.evidence['per_feature_test_results']['stable'] = dict(
            declared=['test_stable'], passed=[], failed=['test_stable'],
            integration_targets_by_test={}, evidence_complete=True)
        self.evidence['evidence_complete'] = True

        report, _ = self.run_all()
        stable = report['features']['stable']
        self.assertTrue(stable.get('review_flagged'))
        self.assertIn('declared tests failed: test_stable', stable['review_reasons'])

    def test_apply_without_apply_demotions_leaves_flagged_stable(self):
        self.features['features']['stable']['open_gaps'] = ['pending']
        report, _ = self.run_all(apply=True)
        self.assertTrue(report['features']['stable'].get('review_flagged'))
        import json
        with open(self.path) as f:
            data = json.load(f)
        self.assertEqual('stable', data['features']['stable']['status'])

    def test_apply_apply_demotions_sets_beta(self):
        import json
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.features['features']['stable']['progress_pct'] = 100
        self.features['features']['stable']['ledger'] = {'promoted_by': 'bot'}

        evidence = self.root / 'evidence.json'
        history = self.root / 'history.json'
        self.path.write_text(json.dumps(self.features))
        evidence.write_text(json.dumps(self.evidence))
        history.write_text(json.dumps(self.history))
        args = ['--features', str(self.path), '--evidence', str(evidence),
                '--history', str(history), '--apply', '--apply-demotions']
        import subprocess, sys
        result = subprocess.run([sys.executable, str(SCRIPTS / 'ledger-promote.py')] + args, capture_output=True, text=True, check=True)
        report = json.loads(result.stdout)
        self.assertIn('stable', report['demotions_applied'])

        with open(self.path) as f:
            data = json.load(f)

        stable = data['features']['stable']
        self.assertEqual('beta', stable['status'])
        self.assertFalse(stable['passes'])
        self.assertIn('demoted stable->beta', stable['notes'])
        self.assertIn('open_gaps present', stable['notes'])
        self.assertEqual(100, stable['progress_pct'])
        self.assertEqual({'promoted_by': 'bot'}, stable['ledger'])

    def test_apply_demotions_without_apply_usage_error(self):
        import json
        evidence = self.root / 'evidence.json'
        history = self.root / 'history.json'
        self.path.write_text(json.dumps(self.features))
        evidence.write_text(json.dumps(self.evidence))
        history.write_text(json.dumps(self.history))
        args = ['--features', str(self.path), '--evidence', str(evidence),
                '--history', str(history), '--apply-demotions']
        import subprocess, sys
        result = subprocess.run([sys.executable, str(SCRIPTS / 'ledger-promote.py')] + args, capture_output=True, text=True)
        self.assertEqual(2, result.returncode)

    def test_metadata_recomputed(self):
        import json
        self.features['metadata'] = {'features_stable': 1, 'features_beta': 0, 'features_in_progress': 0, 'features_planned': 0, 'features_complete': 1, 'total_features': 1, 'overall_progress_pct': 100.0, 'last_verified': '2000-01-01'}
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.features['features']['stable']['progress_pct'] = 100

        evidence = self.root / 'evidence.json'
        history = self.root / 'history.json'
        self.evidence['verified_at'] = '2026-11-10T12:00:00Z'
        self.path.write_text(json.dumps(self.features))
        evidence.write_text(json.dumps(self.evidence))
        history.write_text(json.dumps(self.history))
        args = ['--features', str(self.path), '--evidence', str(evidence),
                '--history', str(history), '--apply', '--apply-demotions']
        import subprocess, sys
        subprocess.run([sys.executable, str(SCRIPTS / 'ledger-promote.py')] + args, check=True)

        with open(self.path) as f:
            data = json.load(f)

        meta = data['metadata']
        self.assertEqual(1, meta.get('features_stable', 0))
        self.assertEqual(2, meta.get('features_beta', 0))
        self.assertEqual(0, meta.get('features_planned', 0))
        self.assertEqual(4, meta.get('total_features', 0))
        self.assertEqual(1, meta.get('features_complete', 0))
        self.assertEqual(60.0, meta.get('overall_progress_pct', 0))
        self.assertEqual('2026-11-10', meta.get('last_verified'))


    def test_workflow_branch_identity(self):
        with open('.github/workflows/ledger-promote.yml') as f:
            workflow = f.read()
        self.assertIn('branch="ledger/promote-$sha-demote"', workflow)

    def test_metadata_initializes_if_absent(self):
        import json
        if 'metadata' in self.features:
            del self.features['metadata']
        self.features['features']['stable']['open_gaps'] = ['pending']
        self.features['features']['stable']['progress_pct'] = 100

        evidence = self.root / 'evidence.json'
        history = self.root / 'history.json'
        self.path.write_text(json.dumps(self.features))
        evidence.write_text(json.dumps(self.evidence))
        history.write_text(json.dumps(self.history))
        args = ['--features', str(self.path), '--evidence', str(evidence),
                '--history', str(history), '--apply', '--apply-demotions']
        import subprocess, sys
        subprocess.run([sys.executable, str(SCRIPTS / 'ledger-promote.py')] + args, check=True)

        with open(self.path) as f:
            data = json.load(f)

        self.assertIn('metadata', data)
        meta = data['metadata']
        self.assertEqual(1, meta.get('features_stable', 0))
        self.assertEqual(2, meta.get('features_beta', 0))
        self.assertEqual(4, meta.get('total_features', 0))
        self.assertEqual(1, meta.get('features_complete', 0))

if __name__ == '__main__':
    unittest.main()
