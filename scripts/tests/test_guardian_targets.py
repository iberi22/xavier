import unittest
import importlib.util
import tempfile
import os
import json
from datetime import datetime, timezone, timedelta

# Import the target validator using importlib
spec = importlib.util.spec_from_file_location(
    "targets", os.path.join(os.path.dirname(__file__), "..", "guardian", "targets.py")
)
targets = importlib.util.module_from_spec(spec)
spec.loader.exec_module(targets)

class TestGuardianTargets(unittest.TestCase):
    def setUp(self):
        self.now = datetime(2023, 10, 20, 12, 0, 0, tzinfo=timezone.utc)

    def test_valid_target(self):
        valid_payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "local_binary",
                    "enabled": True,
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    "health_checked_at": "2023-10-20T11:00:00Z"
                }
            ]
        }
        reasons = targets.validate_inventory(valid_payload, self.now)
        self.assertEqual(reasons, [])

    def test_missing_digest(self):
        payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "local_binary",
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "health_checked_at": "2023-10-20T11:00:00Z"
                }
            ]
        }
        reasons = targets.validate_inventory(payload, self.now)
        self.assertTrue(any("missing or malformed artifact_digest" in r for r in reasons))

    def test_malformed_digest(self):
        payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "local_binary",
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:short",
                    "health_checked_at": "2023-10-20T11:00:00Z"
                }
            ]
        }
        reasons = targets.validate_inventory(payload, self.now)
        self.assertTrue(any("missing or malformed artifact_digest" in r for r in reasons))

    def test_stale_health_evidence(self):
        payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "local_binary",
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    "health_checked_at": "2023-10-19T10:00:00Z" # 26 hours old
                }
            ]
        }
        reasons = targets.validate_inventory(payload, self.now)
        self.assertTrue(any("older than max_evidence_age" in r for r in reasons))

    def test_future_timestamp(self):
        payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "local_binary",
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    "health_checked_at": "2023-10-20T13:00:00Z" # 1 hour in future
                }
            ]
        }
        reasons = targets.validate_inventory(payload, self.now)
        self.assertTrue(any("dated in the future" in r for r in reasons))

    def test_unknown_kind(self):
        payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "unknown_kind",
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    "health_checked_at": "2023-10-20T11:00:00Z"
                }
            ]
        }
        reasons = targets.validate_inventory(payload, self.now)
        self.assertTrue(any("unknown kind: unknown_kind" in r for r in reasons))

    def test_duplicate_id(self):
        payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "local_binary",
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    "health_checked_at": "2023-10-20T11:00:00Z"
                },
                {
                    "id": "target1",
                    "kind": "cf_pages",
                    "production_id": "prod456",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    "health_checked_at": "2023-10-20T11:00:00Z"
                }
            ]
        }
        reasons = targets.validate_inventory(payload, self.now)
        self.assertTrue(any("Duplicate id: target1" in r for r in reasons))

    def test_unresolvable_previous_known_good_id(self):
        payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "local_binary",
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    "health_checked_at": "2023-10-20T11:00:00Z",
                    "previous_known_good_id": "missing_target"
                }
            ]
        }
        reasons = targets.validate_inventory(payload, self.now)
        self.assertTrue(any("does not resolve to another record in the payload" in r for r in reasons))

    def test_load_inventory(self):
        valid_payload = {
            "targets": [
                {
                    "id": "target1",
                    "kind": "local_binary",
                    "production_id": "prod123",
                    "source_sha": "abcdef",
                    "artifact_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    "health_checked_at": "2023-10-20T11:00:00Z"
                }
            ]
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            json_path = os.path.join(tmpdir, "inventory.json")
            with open(json_path, "w", encoding="utf-8") as f:
                json.dump(valid_payload, f)

            loaded = targets.load_inventory(json_path)
            self.assertEqual(loaded, valid_payload)

if __name__ == '__main__':
    unittest.main()
