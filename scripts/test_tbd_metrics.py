import unittest
from unittest.mock import patch, MagicMock
from datetime import datetime, timezone, timedelta
import json
import sys
import os
import io

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

    def test_analyze_cohorts(self):
        now = datetime(2026, 9, 27, 12, 0, 0, tzinfo=timezone.utc)
        prs = [
            {"mergedAt": "2026-09-26T12:00:00Z", "createdAt": "2026-09-25T12:00:00Z", "mergeCommit": {"oid": "abc123def456"}}, # week 0, LT 24h
            {"mergedAt": "2026-09-20T12:00:00Z", "createdAt": "2026-09-18T12:00:00Z", "mergeCommit": {"oid": "safe456"}}, # week 1, LT 48h
        ]
        issues = [
            # Incident mapped by prefix
            {"createdAt": "2026-09-26T10:00:00Z", "closedAt": "2026-09-26T12:00:00Z", "body": "<!-- tbd-incident sha=abc123d workflow=fast-gate -->"},
        ]
        runs = [
            # Run using the new headSha structure and correct workflow name
            {"headSha": "abc123def456", "name": "Fast Gate (Shadow)", "createdAt": "2026-09-26T11:00:00Z", "updatedAt": "2026-09-26T11:05:00Z"} # FG 5 min
        ]

        cohorts = tbd_metrics.analyze_cohorts(prs, issues, runs, 4, now)

        self.assertEqual(len(cohorts[0]['prs']), 1)
        self.assertEqual(cohorts[0]['lead_times'][0], 24.0)
        self.assertEqual(len(cohorts[0]['incidents']), 1)

        # We matched the failed PR because the issue body had "abc123d" (prefix of "abc123def456")
        self.assertIn("abc123d", cohorts[0]['failed_shas'])

        # MTTR: fallback to issue creation time because there was no run_url in the P11 marker
        # closed (12:00) - created (10:00) = 2h
        self.assertEqual(cohorts[0]['mttrs'][0], 2.0)

        # Fast Gate: Run matches by prefix
        self.assertEqual(cohorts[0]['fast_gates'][0], 5.0) # 11:05 - 11:00 = 5m

    @patch('sys.stderr', new_callable=io.StringIO)
    @patch('subprocess.run')
    def test_truncation_warning(self, mock_run, mock_stderr):
        mock_result = MagicMock()
        # Mock 1000 returned items to trigger truncation warning
        mock_result.stdout = json.dumps([{"number": i} for i in range(1000)])
        mock_run.return_value = mock_result

        sys.argv = ['tbd-metrics.py']
        tbd_metrics.main()

        output = mock_stderr.getvalue()
        self.assertIn("WARNING: TRUNCATED PR result limit reached.", output)
        self.assertIn("WARNING: TRUNCATED issues result limit reached.", output)

    @patch('sys.stdout', new_callable=io.StringIO)
    def test_print_metrics_empty(self, mock_stdout):
        cohorts = {0: {'prs': [], 'incidents': [], 'lead_times': [], 'mttrs': [], 'fast_gates': [], 'failed_shas': set()}}
        tbd_metrics.print_metrics(cohorts, 1)
        output = mock_stdout.getvalue()
        self.assertIn("n/a", output)

if __name__ == '__main__':
    unittest.main()
