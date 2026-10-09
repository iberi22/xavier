import unittest
import importlib.util
import tempfile
import os
import json
from datetime import datetime, timezone

spec = importlib.util.spec_from_file_location(
    "probe", os.path.join(os.path.dirname(__file__), "..", "guardian", "probe.py")
)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

class TestGuardianProbe(unittest.TestCase):
    def setUp(self):
        self.now = datetime(2023, 10, 20, 12, 0, 0, tzinfo=timezone.utc)
        self.inventory = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "cf_worker",
                    "enabled": True,
                    "production_id": "v1",
                    "url": "https://example.com"
                }
            ]
        }

    def test_healthy(self):
        def fetch(url, timeout):
            return 200, json.dumps({"deployment_id": "v1"})

        records = probe.probe_targets(self.inventory, "gha", fetch)
        self.assertEqual(len(records), 1)
        self.assertEqual(records[0]["status"], "healthy")
        self.assertEqual(records[0]["deployment_id"], "v1")

    def test_bad_deploy_mismatch(self):
        def fetch(url, timeout):
            return 200, json.dumps({"deployment_id": "v2"})

        records = probe.probe_targets(self.inventory, "gha", fetch)
        self.assertEqual(records[0]["status"], "failing")
        self.assertEqual(records[0]["deployment_id"], "v2")

        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertFalse(evidence[0]["correlated"])

    def test_provider_outage(self):
        def fetch(url, timeout):
            return 0, "timeout"

        records = probe.probe_targets(self.inventory, "gha", fetch)
        self.assertEqual(records[0]["status"], "unknown")

        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertEqual(evidence[0]["status"], "unknown")

    def test_two_vantage_disagreement(self):
        # We simulate two records with different statuses, or we just pass them directly
        # Wait, the prompt says "two-vantage disagreement".
        records = [
            {"target_id": "target1", "deployment_id": "v1", "vantage": "gha", "status": "healthy"},
            {"target_id": "target1", "deployment_id": "v1", "vantage": "aws", "status": "failing"},
        ]
        evidence = probe.correlate(records, self.now)
        # Should be correlated? No, correlated is True only with at least *two agreeing vantages*.
        # Wait, my logic checks `len(vantages) >= 2` where vantages are the ones with "failing" or "unknown".
        self.assertEqual(len(evidence), 1)
        self.assertFalse(evidence[0]["correlated"])

    def test_two_vantage_agreement(self):
        records = [
            {"target_id": "target1", "deployment_id": "v1", "vantage": "gha", "status": "failing"},
            {"target_id": "target1", "deployment_id": "v1", "vantage": "aws", "status": "failing"},
        ]
        evidence = probe.correlate(records, self.now)
        self.assertTrue(evidence[0]["correlated"])

    def test_telemetry_agreement(self):
        records = [
            {"target_id": "target1", "deployment_id": "v1", "vantage": "telemetry", "status": "failing"}
        ]
        evidence = probe.correlate(records, self.now)
        self.assertTrue(evidence[0]["correlated"])

    def test_missing_metrics(self):
        inv = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "cf_worker",
                    "enabled": True,
                    "production_id": "v1",
                    # no url
                }
            ]
        }
        records = probe.probe_targets(inv, "gha", lambda u, t: (0, ""))
        self.assertEqual(records[0]["status"], "unknown")
        self.assertEqual(records[0]["deployment_id"], "v1")

    def test_duplicate_dedupe(self):
        records = [
            {"target_id": "target1", "deployment_id": "v1", "vantage": "gha", "status": "failing"},
            {"target_id": "target1", "deployment_id": "v1", "vantage": "gha", "status": "failing"},
        ]
        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertFalse(evidence[0]["correlated"])

if __name__ == '__main__':
    unittest.main()
