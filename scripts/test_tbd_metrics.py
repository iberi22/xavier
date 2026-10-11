import unittest
from unittest.mock import patch, MagicMock
from datetime import datetime, timezone, timedelta
import json
import sys
import os
import io
import subprocess

import importlib.util

spec = importlib.util.spec_from_file_location("tbd_metrics", os.path.join(os.path.dirname(__file__), "tbd-metrics.py"))
tbd_metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tbd_metrics)

class TestTBDMetrics(unittest.TestCase):
    def test_percentile(self):
        self.assertIsNone(tbd_metrics.percentile([], 50))
        self.assertEqual(tbd_metrics.percentile([1, 2, 3, 4, 5], 50), 3)
        self.assertEqual(tbd_metrics.percentile([1, 2, 3, 4, 5], 90), 4)

    def test_parse_time(self):
        self.assertIsNone(tbd_metrics.parse_time(None))
        dt = tbd_metrics.parse_time("2026-09-27T12:00:00Z")
        self.assertEqual(dt.year, 2026)
        self.assertEqual(dt.tzinfo, timezone.utc)

    def test_get_week_cohort(self):
        now = datetime(2026, 9, 27, 12, 0, 0, tzinfo=timezone.utc)
        self.assertEqual(tbd_metrics.get_week_cohort(now, now), 0)
        self.assertEqual(tbd_metrics.get_week_cohort(now - timedelta(days=7), now), 1)

    def test_extract_incident_data(self):
        body = "Offending SHA: abc123def\nRun URL: https://github.com/run/42"
        sha, url = tbd_metrics.extract_incident_data(body)
        self.assertEqual(sha, "abc123def")
        self.assertEqual(url, "https://github.com/run/42")

        body2 = "No link here"
        sha2, url2 = tbd_metrics.extract_incident_data(body2)
        self.assertIsNone(sha2)
        self.assertIsNone(url2)

        # Test P11 marker
        body3 = "<!-- tbd-incident sha=ff99887 workflow=build-main -->"
        sha3, url3 = tbd_metrics.extract_incident_data(body3)
        self.assertEqual(sha3, "ff99887")
        self.assertIsNone(url3)

    @patch('subprocess.run')
    def test_analyze_cohorts_first_commit(self, mock_run):
        now = datetime(2026, 9, 27, 12, 0, 0, tzinfo=timezone.utc)
        prs = [
            {"number": 1, "mergedAt": "2026-09-26T12:00:00Z", "createdAt": "2026-09-25T12:00:00Z", "mergeCommit": {"oid": "abc123def456"}}, # week 0, LT 24h
            {"number": 2, "mergedAt": "2026-09-20T12:00:00Z", "createdAt": "2026-09-18T12:00:00Z", "mergeCommit": {"oid": "safe456"}}, # week 1, LT 48h
        ]

        # When --first-commit is true, we should hit the API.
        # mock_run is called per PR
        mock_result = MagicMock()
        mock_result.stdout = "2026-09-24T12:00:00Z\n"
        mock_run.return_value = mock_result

        cohorts = tbd_metrics.analyze_cohorts(prs, [], [], 4, now, True, "iberi22", "xavier")

        # For PR 1, merge=26th, first_commit=24th -> LT=48h (default was 24h)
        self.assertEqual(cohorts[0]['lead_times'][0], 48.0)
        # For PR 2, merge=20th, first_commit=24th (mock returns same) -> LT is negative, so max(0, -96) = 0.0
        self.assertEqual(cohorts[1]['lead_times'][0], 0.0)

    def test_analyze_cohorts_no_first_commit(self):
        now = datetime(2026, 9, 27, 12, 0, 0, tzinfo=timezone.utc)
        prs = [
            {"number": 1, "mergedAt": "2026-09-26T12:00:00Z", "createdAt": "2026-09-25T12:00:00Z", "mergeCommit": {"oid": "abc123def456"}},
        ]

        cohorts = tbd_metrics.analyze_cohorts(prs, [], [], 4, now, False, "iberi22", "xavier")

        # For PR 1, merge=26th, createdAt=25th -> LT=24h
        self.assertEqual(cohorts[0]['lead_times'][0], 24.0)

    @patch('sys.stderr', new_callable=io.StringIO)
    @patch('subprocess.run')
    def test_run_gh_cmd_failure(self, mock_run, mock_stderr):
        mock_run.side_effect = subprocess.CalledProcessError(1, ["gh", "pr", "list"], stderr="GraphQL: Exceeded limit")

        with self.assertRaises(SystemExit) as cm:
            tbd_metrics.run_gh_cmd(["gh", "pr", "list"])

        self.assertEqual(cm.exception.code, 2)
        self.assertIn("ERROR: gh pr list failed: GraphQL: Exceeded limit", mock_stderr.getvalue())

if __name__ == '__main__':
    unittest.main()
