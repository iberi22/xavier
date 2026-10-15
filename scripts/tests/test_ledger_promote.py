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
        return json.loads(result.stdout)['features']['beta']

    def blocked(self, reason):
        result = self.run_promoter(apply=True)
        self.assertFalse(result['eligible'])
        self.assertIn(reason, '; '.join(result['reasons']))
        self.assertEqual(self.features, json.loads(self.path.read_text()))

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

    def test_non_beta_with_complete_evidence(self):
        self.assertTrue(self.run_promoter()['eligible'])
        self.features['features']['beta']['status'] = 'in_progress'
        self.blocked('status is not beta')

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


if __name__ == '__main__':
    unittest.main()
