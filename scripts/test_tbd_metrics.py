import unittest
from unittest.mock import patch, MagicMock
from datetime import datetime, timezone, timedelta
import json
import sys
import os

import importlib.util

spec = importlib.util.spec_from_file_location("tbd_metrics", os.path.join(os.path.dirname(__file__), "tbd-metrics.py"))
tbd_metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tbd_metrics)

class TestTBDMetrics(unittest.TestCase):
    def test_percentile(self):
        self.assertEqual(tbd_metrics.percentile([], 50), 0.0)
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

    def test_analyze_cohorts(self):
        now = datetime(2026, 9, 27, 12, 0, 0, tzinfo=timezone.utc)
        prs = [
            {"mergedAt": "2026-09-26T12:00:00Z", "createdAt": "2026-09-25T12:00:00Z", "mergeCommit": {"oid": "abc123def"}}, # week 0, LT 24h
            {"mergedAt": "2026-09-20T12:00:00Z", "createdAt": "2026-09-18T12:00:00Z", "mergeCommit": {"oid": "safe456"}}, # week 1, LT 48h
        ]
        issues = [
            {"createdAt": "2026-09-26T10:00:00Z", "closedAt": "2026-09-26T12:00:00Z", "body": "Offending SHA: abc123def\nRun URL: https://github.com/run/42"}, # week 0, MTTR 2h from issue created, but wait for run...
        ]
        runs = [
            {"url": "https://github.com/run/42", "updatedAt": "2026-09-26T09:00:00Z"}, # Failed run completed at 9am. So MTTR = 12:00 - 09:00 = 3h.
            {"head_sha": "abc123def", "name": "fast-gate", "createdAt": "2026-09-26T11:00:00Z", "updatedAt": "2026-09-26T11:05:00Z"} # FG 5 min
        ]

        cohorts = tbd_metrics.analyze_cohorts(prs, issues, runs, 4, now)

        self.assertEqual(len(cohorts[0]['prs']), 1)
        self.assertEqual(cohorts[0]['lead_times'][0], 24.0)
        self.assertEqual(len(cohorts[0]['incidents']), 1)

        self.assertEqual(cohorts[0]['mttrs'][0], 3.0) # 12:00 closed - 09:00 run updated = 3h
        self.assertEqual(cohorts[0]['fast_gates'][0], 5.0) # 11:05 - 11:00 = 5m

        self.assertIn("abc123def", cohorts[0]['failed_shas'])

        # We can also verify CFR logic manually since it's computed in print_metrics normally,
        # but here we can see the failed_shas matches mergeCommit.

if __name__ == '__main__':
    unittest.main()
