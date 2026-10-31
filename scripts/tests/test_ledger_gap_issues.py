import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import sys
import importlib.util

# Load the script as a module
SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ledger_gap_issues", SCRIPTS / "ledger-gap-issues.py")
lgi = importlib.util.module_from_spec(spec)
sys.modules["ledger_gap_issues"] = lgi
spec.loader.exec_module(lgi)

class FakeRunner:
    def __init__(self, repo, issues_mock=None):
        self.repo = repo
        self.issues = issues_mock or []
        self.calls = []

    def gh(self, *args, **kwargs):
        self.calls.append(args)
        if args[0] == "issue" and args[1] == "list":
            return json.dumps(self.issues)
        return ""

    def create_label_if_missing(self, name, desc, color):
        self.calls.append(("label", "create", name))

class TestLedgerGapIssues(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.features_path = Path(self.temp.name) / "features.json"
        self.report_path = Path(self.temp.name) / "report.json"

    def write_features(self, features):
        self.features_path.write_text(json.dumps({"features": features}))

    def write_report(self, report):
        self.report_path.write_text(json.dumps(report))

    def test_new_gap_creation(self):
        self.write_features({
            "feat-1": {"status": "beta", "name": "F1"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-1": {"reasons": ["declared tests failed: A"]}
            }
        })

        runner = FakeRunner("test/repo")
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, eval_fids)

        creates = [c for c in runner.calls if c[0] == "issue" and c[1] == "create"]
        self.assertEqual(len(creates), 1)
        self.assertIn("--label", creates[0])
        label_idx = list(creates[0]).index("--label")
        self.assertEqual(creates[0][label_idx + 1], "ledger-gap,jules")

    def test_existing_issue_update_only_on_change(self):
        self.write_features({
            "feat-1": {"status": "beta", "name": "F1", "reasons": ["declared tests failed: A"]}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-1": {"reasons": ["declared tests failed: A"]}
            }
        })

        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        body = lgi.format_issue_body("feat-1", gaps["feat-1"])

        runner1 = FakeRunner("test/repo", [{"number": 42, "state": "OPEN", "body": body, "labels": []}])
        lgi.manage_issues(runner1, gaps, sec, ev, sha, 5, False, eval_fids)
        edits1 = [c for c in runner1.calls if c[0] == "issue" and c[1] == "edit"]
        self.assertEqual(len(edits1), 1) # one edit to add jules label

        adds = [c for c in edits1 if "--add-label" in c and "jules" in c]
        self.assertEqual(len(adds), 1)

        runner2 = FakeRunner("test/repo", [{"number": 42, "state": "OPEN", "body": body + "diff", "labels": []}])
        lgi.manage_issues(runner2, gaps, sec, ev, sha, 5, False, eval_fids)
        edits2 = [c for c in runner2.calls if c[0] == "issue" and c[1] == "edit"]
        self.assertEqual(len(edits2), 2)
        adds2 = [c for c in edits2 if "--add-label" in c and "jules" in c]
        self.assertEqual(len(adds2), 1)

    def test_close_on_stable(self):
        self.write_features({
            "feat-1": {"status": "stable", "name": "F1"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {"feat-1": {"review_flagged": False, "review_reasons": []}}
        })

        runner = FakeRunner("test/repo", [{"number": 42, "state": "OPEN", "body": "<!-- ledger-gap:feat-1 -->"}])
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, eval_fids)

        closes = [c for c in runner.calls if c[0] == "issue" and c[1] == "close"]
        self.assertEqual(len(closes), 1)
        comment_idx = list(closes[0]).index("--comment")
        self.assertIn("123", closes[0][comment_idx+1])

    def test_reopen_and_jules_limit(self):
        self.write_features({
            "feat-1": {"status": "beta", "name": "F1"},
            "feat-2": {"status": "beta", "name": "F2"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-1": {"reasons": ["declared tests failed: A"]},
                "feat-2": {"reasons": ["declared tests failed: B"]},
            }
        })

        issues = [
            {"number": 1, "state": "CLOSED", "body": "<!-- ledger-gap:feat-1 -->"},
            {"number": 2, "state": "CLOSED", "body": "<!-- ledger-gap:feat-2 -->", "labels": [{"name": "jules"}]},
        ]

        runner = FakeRunner("test/repo", issues)
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)

        lgi.manage_issues(runner, gaps, sec, ev, sha, 1, False, eval_fids)

        reopens = [c for c in runner.calls if c[0] == "issue" and c[1] == "reopen"]
        self.assertEqual(len(reopens), 2)

        adds = [c for c in runner.calls if c[0] == "issue" and "--add-label" in c and "jules" in c]
        self.assertEqual(len(adds), 1)

        removes = [c for c in runner.calls if c[0] == "issue" and "--remove-label" in c and "jules" in c]
        self.assertEqual(len(removes), 1)

    def test_security_category_skipped(self):
        self.write_features({
            "feat-sec": {"status": "beta", "category": "security"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-sec": {"reasons": ["declared tests failed: S"]}
            }
        })

        runner = FakeRunner("test/repo", [{"number": 99, "state": "OPEN", "body": "<!-- ledger-gap:feat-sec -->", "labels": []}])
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, eval_fids)

        self.assertIn("feat-sec", sec)
        self.assertNotIn("feat-sec", gaps)

        modifications = [c for c in runner.calls if c[0] == "issue" and c[1] in ("create", "edit", "close", "reopen")]
        self.assertEqual(len(modifications), 0)

    def test_streak_only_ignored(self):
        self.write_features({"feat-1": {"status": "beta"}})
        self.write_report({
            "features": {"feat-1": {"reasons": ["green streak 1 < 3"]}}
        })
        gaps, _, _, _, _ = lgi.find_gaps(self.features_path, self.report_path)
        self.assertNotIn("feat-1", gaps)

    def test_report_membership_is_not_resolution(self):
        for status in ("stable", "beta"):
            for result in (
                {},
                {"review_flagged": True, "review_reasons": ["declared tests failed: A"]},
                {"review_flagged": False, "review_reasons": ["review skipped: missing evidence"]},
                {"reasons": ["declared tests failed: A"]},
            ):
                with self.subTest(status=status, result=result):
                    self.write_features({"feat-1": {"status": status}})
                    self.write_report({"evidence_complete": True, "features": {"feat-1": result}})
                    runner = FakeRunner("test/repo", [{
                        "number": 42, "state": "OPEN", "body": "<!-- ledger-gap:feat-1 -->"
                    }])
                    gaps, sec, ev, sha, gap_free = lgi.find_gaps(self.features_path, self.report_path)
                    lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, gap_free)
                    self.assertFalse(any(c[:2] == ("issue", "close") for c in runner.calls))

    def test_eligible_gap_free_feature_closes(self):
        self.write_features({"feat-1": {"status": "beta"}})
        self.write_report({"evidence_complete": True, "features": {
            "feat-1": {"eligible": True, "reasons": []}
        }})
        runner = FakeRunner("test/repo", [{
            "number": 42, "state": "OPEN", "body": "<!-- ledger-gap:feat-1 -->"
        }])
        gaps, sec, ev, sha, gap_free = lgi.find_gaps(self.features_path, self.report_path)
        lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, gap_free)
        self.assertTrue(any(c[:2] == ("issue", "close") for c in runner.calls))

    def test_incomplete_evidence_makes_no_calls(self):
        runner = FakeRunner("test/repo")
        lgi.manage_issues(runner, {}, set(), False, "123", 5, False, {"feat-1"})
        self.assertEqual(runner.calls, [])

    def test_main_red_no_creates(self):
        self.write_features({"feat-1": {"status": "beta"}})
        self.write_report({
            "evidence_complete": True,
            "features": {"feat-1": {"reasons": ["declared tests failed: A"]}}
        })
        runner = FakeRunner("test/repo")
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        lgi.manage_issues(runner, gaps, sec, ev, sha, 5, True, eval_fids)

        creates = [c for c in runner.calls if c[0] == "issue" and c[1] == "create"]
        self.assertEqual(len(creates), 0)

    def test_missing_evidence_leaves_issues_untouched(self):
        self.write_features({
            "feat-1": {"status": "stable", "name": "F1"} # Should be closed normally
        })
        self.write_report({
            "evidence_complete": False,
            "sha": "123",
            "features": {"feat-1": {}}
        })

        runner = FakeRunner("test/repo", [{"number": 42, "state": "OPEN", "body": "<!-- ledger-gap:feat-1 -->"}])
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, eval_fids)

        closes = [c for c in runner.calls if c[0] == "issue" and c[1] == "close"]
        self.assertEqual(len(closes), 0)

    def test_jules_budget_on_creation(self):
        self.write_features({
            "feat-1": {"status": "beta", "name": "F1"},
            "feat-2": {"status": "beta", "name": "F2"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-1": {"reasons": ["declared tests failed: A"]},
                "feat-2": {"reasons": ["declared tests failed: B"]}
            }
        })

        runner = FakeRunner("test/repo")
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        lgi.manage_issues(runner, gaps, sec, ev, sha, 1, False, eval_fids)

        creates = [c for c in runner.calls if c[0] == "issue" and c[1] == "create"]
        self.assertEqual(len(creates), 2)

        labels_applied = 0
        for c in creates:
            label_idx = list(c).index("--label")
            if "jules" in c[label_idx + 1]:
                labels_applied += 1

        self.assertEqual(labels_applied, 1)

    def test_implemented_in_normalization(self):
        self.write_features({
            "feat-1": {"status": "beta", "name": "F1", "implemented_in": "src/only.rs"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-1": {"reasons": ["declared tests failed: A"]}
            }
        })

        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        self.assertEqual(gaps["feat-1"]["implemented_in"], ["src/only.rs"])



    def test_security_issue_labels_skipped(self):
        self.write_features({
            "feat-sec": {"status": "beta", "name": "Security Feature"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-sec": {"reasons": ["declared tests failed: S"]}
            }
        })
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        for label in ("security", "security-report"):
            for state in ("OPEN", "CLOSED"):
                with self.subTest(label=label, state=state):
                    runner = FakeRunner("test/repo", [{"number": 99, "state": state,
                        "body": "<!-- ledger-gap:feat-sec -->", "labels": [{"name": label}]}])
                    lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, eval_fids)
                    modifications = [c for c in runner.calls if c[0] == "issue" and c[1] in ("create", "edit", "close", "reopen")]
                    self.assertEqual(modifications, [])



    def test_dry_run_makes_no_calls(self):
        self.write_features({
            "feat-1": {"status": "beta", "name": "F1"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-1": {"reasons": ["declared tests failed: A"]}
            }
        })
        runner = lgi.GithubRunner("test/repo", dry_run=True)
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        with patch('subprocess.run') as mock_run:
            lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, eval_fids)
            mock_run.assert_not_called()

# if __name__ == '__main__':
#     unittest.main()

    def test_reopen_with_jules_label_consumes_budget(self):
        self.write_features({
            "feat-1": {"status": "beta", "name": "F1"},
            "feat-2": {"status": "beta", "name": "F2"}
        })
        self.write_report({
            "evidence_complete": True,
            "sha": "123",
            "features": {
                "feat-1": {"reasons": ["declared tests failed: A"]},
                "feat-2": {"reasons": ["declared tests failed: B"]},
            }
        })
        # F1 closed with jules, F2 open without
        issues = [
            {"number": 1, "state": "CLOSED", "body": "<!-- ledger-gap:feat-1 -->", "labels": [{"name": "jules"}]},
            {"number": 2, "state": "OPEN", "body": "<!-- ledger-gap:feat-2 -->", "labels": []},
        ]
        runner = FakeRunner("test/repo", issues)
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)

        # Budget of 1. Because we process F1 (which removes and re-adds jules),
        # budget should be consumed, so F2 should NOT get jules.
        lgi.manage_issues(runner, gaps, sec, ev, sha, 1, False, eval_fids)

        adds = [c for c in runner.calls if c[0] == "issue" and "--add-label" in c and "jules" in c]
        # Only 1 jules label added (to issue 1, not 2)
        self.assertEqual(len(adds), 1)
        self.assertIn("1", adds[0])
        removes = [c for c in runner.calls if "--remove-label" in c]
        self.assertEqual(len(removes), 1)
        self.assertEqual(removes[0][2], "1")
        self.assertLess(runner.calls.index(removes[0]), runner.calls.index(adds[0]))


class TestPromoterRunProvenance(unittest.TestCase):
    def test_provenance_checked_before_download(self):
        workflow = (SCRIPTS.parent / '.github/workflows/ledger-gaps.yml').read_text()
        self.assertLess(workflow.index('actions/runs/$RUN_ID'), workflow.index('actions/download-artifact'))
        # Execute the actual validation block with a fake gh; no network calls.
        block = workflow.split('        run: |\n', 1)[1].split('\n      - uses:', 1)[0]
        block = '\n'.join(line[10:] for line in block.splitlines())
        self.assertNotIn('${{', block)
        fake_gh = '''gh() {
          echo call >> "$CALL_LOG"
          case "$2" in
            */git/ref/heads/main) echo current-main ;;
            */actions/runs/*) echo "$RUN_JSON" ;;
            *) return 1 ;;
          esac
        }
        '''
        valid = dict(name='Ledger Promoter', conclusion='success', head_branch='main', head_sha='current-main')
        cases = [(valid, '123', True)]
        for field, value in (('name', 'Other workflow'), ('conclusion', 'failure'),
                             ('head_branch', 'topic'), ('head_sha', 'stale')):
            cases.append((dict(valid, **{field: value}), '123', False))
        for run_id in ('', 'abc', '123/other', '123\nid=456', '$(touch injected)'):
            cases.append((valid, run_id, False))
        with tempfile.TemporaryDirectory() as tmp:
            for event in ('workflow_dispatch', 'workflow_run'):
                for run, run_id, succeeds in cases:
                    with self.subTest(event=event, run=run, run_id=run_id):
                        output, calls = Path(tmp) / 'output', Path(tmp) / 'calls'
                        output.write_text('')
                        calls.write_text('')
                        env = dict(os.environ, EVENT_NAME=event, INPUT_RUN_ID=run_id,
                                   EVENT_RUN_ID=run_id, GITHUB_REPOSITORY='test/repo',
                                   RUN_JSON=json.dumps(run), GITHUB_OUTPUT=str(output), CALL_LOG=str(calls))
                        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', fake_gh + block],
                                                env=env, cwd=tmp, capture_output=True, text=True)
                        self.assertEqual(result.returncode == 0, succeeds, result.stderr)
                        self.assertEqual(bool(output.read_text()), succeeds)
                        if not run_id.isascii() or not run_id.isdigit():
                            self.assertEqual(calls.read_text(), '')

# if __name__ == '__main__':
#     unittest.main()
if __name__ == '__main__':
    unittest.main()
