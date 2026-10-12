import importlib.util
from io import StringIO
import os
import sys
import tempfile
import unittest
from unittest.mock import patch
import json
import datetime

class TestShadowGateEvidence(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = os.path.join(os.path.dirname(os.path.dirname(__file__)), "shadow-gate-evidence.py")
        spec = importlib.util.spec_from_file_location("shadow_gate_evidence", path)
        cls.mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.mod)

    def run_main(self, args_dict, gh_res=None):
        gh_map = gh_res or {}
        def mock_gh(cmd, as_text=False):
            s = " ".join(cmd)
            for k, v in gh_map.items():
                if k in s:
                    if isinstance(v, Exception):
                        raise v
                    return v
            return [] if not as_text else ""

        args = type("Args", (), args_dict)()
        with patch("sys.stdout", new_callable=StringIO) as out_buf:
            self.mod.main(args, mock_gh)
            return out_buf.getvalue()

    def test_missed_failure(self):
        pr_sha = "abcdef123"
        prs = [{"number": 100, "headRefOid": pr_sha}]
        shadow_runs = [{"databaseId": 1, "headSha": pr_sha, "createdAt": "2026-10-10T10:00:00Z", "updatedAt": "2026-10-10T10:05:00Z", "startedAt": "2026-10-10T10:01:00Z"}]
        ci_runs = [{"databaseId": 2, "headSha": pr_sha, "conclusion": "failure", "createdAt": "2026-10-10T10:00:00Z", "updatedAt": "2026-10-10T10:10:00Z"}]
        jobs = {"jobs": [{"name": "test-rust", "conclusion": "failure"}]}
        log_txt = '::notice::classify={"class": "flow:ship"}\n::notice::rust-impact={"skip_rust": true}'

        gh_res = {
            "gh pr list": prs,
            "gh run list --workflow fast-gate.yml": shadow_runs,
            "gh run list --workflow ci.yml": ci_runs,
            "gh run view 2 --json jobs": jobs,
            "gh run view 1 --log": log_txt
        }

        args = {"since": "2026-10-01", "until": None, "out": None, "json": False}
        with patch.object(self.mod, 'datetime', spec=datetime.datetime) as mock_datetime:
            mock_datetime.now.return_value = datetime.datetime(2026, 10, 20, tzinfo=datetime.timezone.utc)
            mock_datetime.fromisoformat.side_effect = datetime.datetime.fromisoformat
            mock_datetime.min = datetime.datetime.min
            out = self.run_main(args, gh_res)

        self.assertIn("Missed failures", out)
        self.assertIn("abcdef123", out)
        self.assertIn("100", out)

    def test_unclassified_pr(self):
        pr_sha = "abcdef123"
        prs = [{"number": 100, "headRefOid": pr_sha}]
        shadow_runs = [{"databaseId": 1, "headSha": pr_sha, "createdAt": "2026-10-10T10:00:00Z", "updatedAt": "2026-10-10T10:05:00Z", "startedAt": "2026-10-10T10:01:00Z"}]
        ci_runs = []
        log_txt = 'some random log without notices'

        gh_res = {
            "gh pr list": prs,
            "gh run list --workflow fast-gate.yml": shadow_runs,
            "gh run list --workflow ci.yml": ci_runs,
            "gh run view 1 --log": log_txt
        }

        args = {"since": "2026-10-01", "until": None, "out": None, "json": False}
        with patch.object(self.mod, 'datetime', spec=datetime.datetime) as mock_datetime:
            mock_datetime.now.return_value = datetime.datetime(2026, 10, 20, tzinfo=datetime.timezone.utc)
            mock_datetime.fromisoformat.side_effect = datetime.datetime.fromisoformat
            mock_datetime.min = datetime.datetime.min
            out = self.run_main(args, gh_res)
        self.assertIn("Unclassified count: 1", out)
        self.assertIn("PANEL_INPUT: insufficient", out)

    def test_insufficient_window(self):
        args = {"since": "2026-10-01", "until": None, "out": None, "json": False}
        with patch.object(self.mod, 'datetime', spec=datetime.datetime) as mock_datetime:
            mock_datetime.now.return_value = datetime.datetime(2026, 10, 5, tzinfo=datetime.timezone.utc)
            mock_datetime.fromisoformat.side_effect = datetime.datetime.fromisoformat
            mock_datetime.min = datetime.datetime.min
            out = self.run_main(args, {})
        self.assertIn("PANEL_INPUT: insufficient", out)

    def test_latency_calculation(self):
        pr_sha = "abcdef123"
        prs = [{"number": 100, "headRefOid": pr_sha}]
        # queue=1m, duration=4m, total=5m (300s). run_duration=240s
        shadow_runs = [{"databaseId": 1, "headSha": pr_sha, "createdAt": "2026-10-10T10:00:00Z", "updatedAt": "2026-10-10T10:05:00Z", "startedAt": "2026-10-10T10:01:00Z"}]
        log_txt = '::notice::classify={"class": "flow:ship"}\n::notice::rust-impact={"skip_rust": false}'

        gh_res = {
            "gh pr list": prs,
            "gh run list --workflow fast-gate.yml": shadow_runs,
            "gh run list --workflow ci.yml": [],
            "gh run view 1 --log": log_txt
        }

        args = {"since": "2026-10-01", "until": None, "out": None, "json": False}
        with patch.object(self.mod, 'datetime', spec=datetime.datetime) as mock_datetime:
            mock_datetime.now.return_value = datetime.datetime(2026, 10, 20, tzinfo=datetime.timezone.utc)
            mock_datetime.fromisoformat.side_effect = datetime.datetime.fromisoformat
            mock_datetime.min = datetime.datetime.min
            out = self.run_main(args, gh_res)

        self.assertIn("p50=300.0", out)
        self.assertIn("p50=240.0", out)
        self.assertIn("PANEL_INPUT: ready", out)

    def test_multiline_nested_rust_impact_payload(self):
        pr_sha = "abcdef123"
        prs = [{"number": 100, "headRefOid": pr_sha}]
        shadow_runs = [{"databaseId": 1, "headSha": pr_sha, "createdAt": "2026-10-10T10:00:00Z", "updatedAt": "2026-10-10T10:05:00Z", "startedAt": "2026-10-10T10:01:00Z"}]
        nested_impact = {
            "fallback": False,
            "reason": "Scoped selection based on changed files",
            "packages": [
                {
                    "name": "code-graph",
                    "targets": ["code_graph"]
                },
                {
                    "name": "xavier",
                    "targets": [
                        "auth_register_test",
                        "xavier"
                    ]
                }
            ]
        }
        log_txt = (
            '::notice::classify={"class": "flow:ship"}\n'
            '::notice::rust-impact=' + json.dumps(nested_impact, indent=2) + '\n'
            'Always-present summary check completed'
        )

        gh_res = {
            "gh pr list": prs,
            "gh run list --workflow fast-gate.yml": shadow_runs,
            "gh run list --workflow ci.yml": [],
            "gh run view 1 --log": log_txt
        }

        # Verify extract_log_notices parses multi-line nested payload without truncating at nested braces
        def mock_gh(cmd, as_text=False):
            return log_txt

        classify, rust_impact = self.mod.extract_log_notices(1, mock_gh)
        self.assertEqual(classify, {"class": "flow:ship"})
        self.assertEqual(rust_impact, nested_impact)

        args = {"since": "2026-10-01", "until": None, "out": None, "json": False}
        with patch.object(self.mod, 'datetime', spec=datetime.datetime) as mock_datetime:
            mock_datetime.now.return_value = datetime.datetime(2026, 10, 20, tzinfo=datetime.timezone.utc)
            mock_datetime.fromisoformat.side_effect = datetime.datetime.fromisoformat
            mock_datetime.min = datetime.datetime.min
            out = self.run_main(args, gh_res)

        self.assertIn("Unclassified count: 0", out)
        self.assertIn("PANEL_INPUT: ready", out)

    def test_multiline_skip_rust_missed_failure(self):
        pr_sha = "abcdef123"
        prs = [{"number": 100, "headRefOid": pr_sha}]
        shadow_runs = [{"databaseId": 1, "headSha": pr_sha, "createdAt": "2026-10-10T10:00:00Z", "updatedAt": "2026-10-10T10:05:00Z", "startedAt": "2026-10-10T10:01:00Z"}]
        ci_runs = [{"databaseId": 2, "headSha": pr_sha, "conclusion": "failure", "createdAt": "2026-10-10T10:00:00Z", "updatedAt": "2026-10-10T10:10:00Z"}]
        jobs = {"jobs": [{"name": "test-rust", "conclusion": "failure"}]}
        nested_impact = {
            "skip_rust": True,
            "reason": "All changed files are known non-Rust paths",
            "packages": []
        }
        log_txt = (
            '::notice::classify={"class": "flow:ship"}\n'
            '::notice::rust-impact=' + json.dumps(nested_impact, indent=2) + '\n'
        )

        gh_res = {
            "gh pr list": prs,
            "gh run list --workflow fast-gate.yml": shadow_runs,
            "gh run list --workflow ci.yml": ci_runs,
            "gh run view 2 --json jobs": jobs,
            "gh run view 1 --log": log_txt
        }

        args = {"since": "2026-10-01", "until": None, "out": None, "json": False}
        with patch.object(self.mod, 'datetime', spec=datetime.datetime) as mock_datetime:
            mock_datetime.now.return_value = datetime.datetime(2026, 10, 20, tzinfo=datetime.timezone.utc)
            mock_datetime.fromisoformat.side_effect = datetime.datetime.fromisoformat
            mock_datetime.min = datetime.datetime.min
            out = self.run_main(args, gh_res)

        self.assertIn("Skip Rust decisions: 1", out)
        self.assertIn("Missed failures", out)
        self.assertIn("abcdef123", out)

if __name__ == "__main__":
    unittest.main()
