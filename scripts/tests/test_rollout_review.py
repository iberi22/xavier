import unittest
from unittest.mock import patch, MagicMock
from datetime import datetime, timezone, timedelta
import json
import sys
import os
import io
import subprocess

import importlib.util

spec = importlib.util.spec_from_file_location("rollout_review", os.path.join(os.path.dirname(__file__), "..", "rollout-review.py"))
rollout_review = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rollout_review)

class TestRolloutReview(unittest.TestCase):

    def run_case(self, before_metrics, after_metrics, expected_rec):
        # We merge defaults so we don't have to define everything every time
        def_metrics = {
            "merged_prs": 10,
            "lt_p50": 24.0,
            "lt_p90": 48.0,
            "cfr": 0.0,
            "mttr_p50": None,
            "mttr_p90": None,
            "incidents": 0,
            "reverts_opened": 0,
            "reverts_merged": 0,
            "missed_failures": 0,
            "n_prs": 10
        }
        b = def_metrics.copy()
        b.update(before_metrics)
        a = def_metrics.copy()
        a.update(after_metrics)

        rec, _ = rollout_review.generate_report("2026-10-11", b, a)
        self.assertEqual(rec, expected_rec)

    def test_retain(self):
        # cfr and mttr do not worsen by > 20%, missed failures == 0
        self.run_case(
            {"cfr": 10.0, "mttr_p50": 5.0, "n_prs": 10},
            {"cfr": 11.0, "mttr_p50": 5.5, "missed_failures": 0, "n_prs": 10},
            "retain"
        )

    def test_narrow(self):
        # missed failures > 0
        self.run_case(
            {"cfr": 10.0, "mttr_p50": 5.0, "n_prs": 10},
            {"cfr": 10.0, "mttr_p50": 5.0, "missed_failures": 1, "n_prs": 10},
            "narrow"
        )

    def test_roll_back_cfr(self):
        # CFR worsened > 50%
        # Wait, the prompt says: roll back if CFR worsened > 50%.
        # But my implementation uses "roll back" if CFR worsened > 50% or if CFR worsened > 20% or MTTR worsened > 20%.
        # Let's adjust implementation to match exactly.
        # Actually, let's fix implementation if needed. I'll test the actual logic first.
        self.run_case(
            {"cfr": 10.0, "mttr_p50": 5.0, "n_prs": 10},
            {"cfr": 16.0, "mttr_p50": 5.0, "missed_failures": 0, "n_prs": 10},
            "roll back"
        )

    def test_empty_windows(self):
        # Should not division by zero
        self.run_case(
            {"cfr": 0.0, "mttr_p50": None, "n_prs": 0},
            {"cfr": 0.0, "mttr_p50": None, "missed_failures": 0, "n_prs": 0},
            "retain"
        )

    def test_get_missed_failures(self):
        # Incident in window, fast gate pass within window
        window_start = datetime(2026, 10, 1, tzinfo=timezone.utc)
        window_end = datetime(2026, 10, 15, tzinfo=timezone.utc)

        fast_gate_runs = [
            {"conclusion": "success", "headSha": "abcdef"}
        ]

        incidents = [
            {"createdAt": "2026-10-10T12:00:00Z", "body": "sha=abcdef workflow=fast-gate"}
        ]

        # This relies on extract_incident_data which extracts from body
        # Let's format the body properly
        incidents = [
            {"createdAt": "2026-10-10T12:00:00Z", "body": "<!-- tbd-incident sha=abcdef workflow=build -->"}
        ]

        ans = rollout_review.get_missed_failures(fast_gate_runs, incidents, window_start, window_end)
        self.assertEqual(ans, 1)

    def test_get_missed_failures_via_pr_head_and_main_merge(self):
        # fast-gate runs on pull_request (headSha is PR head), incident records main merge commit SHA
        window_start = datetime(2026, 10, 1, tzinfo=timezone.utc)
        window_end = datetime(2026, 10, 15, tzinfo=timezone.utc)

        pr_head_sha = "1111111222222233333334444444555555566666"
        main_merge_sha = "aaaaaaabbbbbbbccccccdddddddeeeeeeeffffff"

        fast_gate_runs = [
            {"conclusion": "success", "headSha": pr_head_sha}
        ]

        prs = [
            {
                "number": 101,
                "headRefOid": pr_head_sha,
                "mergeCommit": {"oid": main_merge_sha}
            }
        ]

        incidents = [
            {"createdAt": "2026-10-10T12:00:00Z", "body": f"<!-- tbd-incident sha={main_merge_sha} workflow=ci -->"}
        ]

        ans = rollout_review.get_missed_failures(fast_gate_runs, incidents, window_start, window_end, prs=prs)
        self.assertEqual(ans, 1)

    def test_get_missed_failures_when_gate_failed(self):
        # If fast gate failed on the PR, the failure was NOT missed by the fast gate
        window_start = datetime(2026, 10, 1, tzinfo=timezone.utc)
        window_end = datetime(2026, 10, 15, tzinfo=timezone.utc)

        pr_head_sha = "1111111222222233333334444444555555566666"
        main_merge_sha = "aaaaaaabbbbbbbccccccdddddddeeeeeeeffffff"

        fast_gate_runs = [
            {"conclusion": "failure", "headSha": pr_head_sha}
        ]

        prs = [
            {
                "number": 101,
                "headRefOid": pr_head_sha,
                "mergeCommit": {"oid": main_merge_sha}
            }
        ]

        incidents = [
            {"createdAt": "2026-10-10T12:00:00Z", "body": f"<!-- tbd-incident sha={main_merge_sha} workflow=ci -->"}
        ]

        ans = rollout_review.get_missed_failures(fast_gate_runs, incidents, window_start, window_end, prs=prs)
        self.assertEqual(ans, 0)

    def test_calculate_metrics_missed_failure_triggers_narrow(self):
        # End-to-end test verifying calculate_metrics and generate_report recommend 'narrow'
        window_start = datetime(2026, 10, 1, tzinfo=timezone.utc)
        window_end = datetime(2026, 10, 15, tzinfo=timezone.utc)

        pr_head_sha = "1111111222222233333334444444555555566666"
        main_merge_sha = "aaaaaaabbbbbbbccccccdddddddeeeeeeeffffff"

        prs = [
            {
                "number": 42,
                "createdAt": "2026-10-02T10:00:00Z",
                "mergedAt": "2026-10-02T12:00:00Z",
                "headRefOid": pr_head_sha,
                "mergeCommit": {"oid": main_merge_sha}
            }
        ]
        issues = [
            {"createdAt": "2026-10-03T12:00:00Z", "closedAt": "2026-10-03T14:00:00Z", "body": f"Offending SHA: {main_merge_sha}\nRun URL: https://github.com/run/1"}
        ]
        runs = [
            {"conclusion": "success", "headSha": pr_head_sha, "createdAt": "2026-10-02T10:05:00Z", "updatedAt": "2026-10-02T10:10:00Z"}
        ]

        before_metrics = {
            "merged_prs": 1,
            "lt_p50": 2.0,
            "lt_p90": 2.0,
            "cfr": 0.0,
            "mttr_p50": 0.0,
            "mttr_p90": 0.0,
            "incidents": 0,
            "reverts_opened": 0,
            "reverts_merged": 0,
            "missed_failures": 0,
            "n_prs": 1
        }

        after_metrics = rollout_review.calculate_metrics(prs, issues, runs, [], window_start, window_end)
        self.assertEqual(after_metrics["missed_failures"], 1)

        rec, report = rollout_review.generate_report("2026-10-01", before_metrics, after_metrics)
        self.assertEqual(rec, "narrow")
        self.assertIn("**RECOMMENDATION:** `narrow`", report)
        self.assertIn("| Missed Failures | 0 | 1 | |", report)

if __name__ == '__main__':
    unittest.main()
