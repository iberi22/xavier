import json
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
            "features": {"feat-1": {}}
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
        runner = FakeRunner("test/repo", [{"number": 99, "state": "OPEN", "body": "<!-- ledger-gap:feat-sec -->", "labels": [{"name": "security"}]}])
        gaps, sec, ev, sha, eval_fids = lgi.find_gaps(self.features_path, self.report_path)
        lgi.manage_issues(runner, gaps, sec, ev, sha, 5, False, eval_fids)

        modifications = [c for c in runner.calls if c[0] == "issue" and c[1] in ("create", "edit", "close", "reopen")]
        self.assertEqual(len(modifications), 0)
        # Verify it wasn't re-created because it was correctly marked as handled
        creates = [c for c in runner.calls if c[0] == "issue" and c[1] == "create"]
        self.assertEqual(len(creates), 0)



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

# if __name__ == '__main__':
#     unittest.main()
if __name__ == '__main__':
    unittest.main()
