import unittest
import importlib.util
import os
import json
from datetime import datetime, timezone

spec = importlib.util.spec_from_file_location(
    "probe", os.path.join(os.path.dirname(__file__), "..", "guardian", "probe.py")
)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

PROBE_KEYS = {"target_id", "kind", "deployment_id", "vantage", "observed_at",
              "status", "checks", "error"}
EVIDENCE_KEYS = {"target_id", "kind", "deployment_id", "detected_at", "failed_probes",
                 "window_seconds", "vantages", "correlated", "missing_metrics", "summary"}

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

    def assertProbeRecord(self, record):
        self.assertEqual(set(record.keys()), PROBE_KEYS)
        self.assertIn(record["status"], ("healthy", "unhealthy", "unknown"))
        self.assertTrue(record["observed_at"].endswith("Z"))
        self.assertIsInstance(record["checks"], list)

    def assertFailureEvidence(self, record):
        self.assertEqual(set(record.keys()), EVIDENCE_KEYS)
        self.assertIsInstance(record["vantages"], list)
        self.assertIsInstance(record["failed_probes"], list)
        self.assertIsInstance(record["missing_metrics"], list)
        self.assertIsInstance(record["correlated"], bool)
        self.assertTrue(record["detected_at"].endswith("Z"))

    def test_healthy(self):
        def fetch(url, timeout):
            return 200, json.dumps({"deployment_id": "v1"})

        records = probe.probe_targets(self.inventory, "gha", fetch)
        self.assertEqual(len(records), 1)
        self.assertProbeRecord(records[0])
        self.assertEqual(records[0]["status"], "healthy")
        self.assertEqual(records[0]["kind"], "cf_worker")
        self.assertEqual(records[0]["deployment_id"], "v1")
        self.assertIsNone(records[0]["error"])
        self.assertTrue(records[0]["checks"][0]["ok"])

    def test_bad_deploy_mismatch(self):
        def fetch(url, timeout):
            return 200, json.dumps({"deployment_id": "v2"})

        records = probe.probe_targets(self.inventory, "gha", fetch)
        self.assertProbeRecord(records[0])
        self.assertEqual(records[0]["status"], "unhealthy")
        self.assertEqual(records[0]["deployment_id"], "v2")

        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertFailureEvidence(evidence[0])
        self.assertFalse(evidence[0]["correlated"])

    def test_provider_outage(self):
        def fetch(url, timeout):
            return 0, "timeout"

        records = probe.probe_targets(self.inventory, "gha", fetch)
        self.assertProbeRecord(records[0])
        self.assertEqual(records[0]["status"], "unknown")
        self.assertIsNotNone(records[0]["error"])

        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertFailureEvidence(evidence[0])
        self.assertEqual(evidence[0]["missing_metrics"], ["gha"])
        self.assertEqual(evidence[0]["failed_probes"], [])
        self.assertFalse(evidence[0]["correlated"])

    def test_two_vantage_disagreement(self):
        records = [
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "gha", "status": "healthy"},
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "aws", "status": "unhealthy"},
        ]
        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertFailureEvidence(evidence[0])
        self.assertFalse(evidence[0]["correlated"])

    def test_two_vantage_agreement(self):
        records = [
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "gha", "status": "unhealthy"},
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "aws", "status": "unhealthy"},
        ]
        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertTrue(evidence[0]["correlated"])

    def test_telemetry_agreement(self):
        records = [
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "telemetry", "status": "unhealthy"}
        ]
        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
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
        self.assertProbeRecord(records[0])
        self.assertEqual(records[0]["status"], "unknown")
        self.assertEqual(records[0]["deployment_id"], "v1")
        evidence = probe.correlate(records, self.now)
        self.assertEqual(evidence[0]["missing_metrics"], ["gha"])

    def test_duplicate_dedupe(self):
        records = [
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "gha", "status": "unhealthy"},
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "gha", "status": "unhealthy"},
        ]
        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertFailureEvidence(evidence[0])
        self.assertFalse(evidence[0]["correlated"])

    def test_contract_unhealthy_status_is_visible(self):
        # Regression: the contract enum uses "unhealthy"; it must not be dropped.
        records = [
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "gha", "status": "unhealthy"},
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "local", "status": "unhealthy"},
        ]
        evidence = probe.correlate(records, self.now)
        self.assertEqual(len(evidence), 1)
        self.assertFailureEvidence(evidence[0])
        self.assertTrue(evidence[0]["correlated"])

    def test_correlate_does_not_mutate_input(self):
        records = [
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "gha", "status": "unhealthy"},
            {"target_id": "target1", "kind": "cf_worker", "deployment_id": "v1",
             "vantage": "local", "status": "unhealthy"},
        ]
        import copy
        snapshot = copy.deepcopy(records)
        probe.correlate(records, self.now)
        self.assertEqual(records, snapshot)

if __name__ == '__main__':
    unittest.main()
