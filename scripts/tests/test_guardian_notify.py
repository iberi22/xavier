import unittest
import importlib.util
import tempfile
import os
import json
from datetime import datetime, timezone

# Load notify.py module
spec = importlib.util.spec_from_file_location(
    "notify", os.path.join(os.path.dirname(__file__), "..", "guardian", "notify.py")
)
notify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notify)

class TestGuardianNotify(unittest.TestCase):
    def setUp(self):
        self.now = datetime(2023, 10, 20, 12, 0, 0, tzinfo=timezone.utc)

    def test_deduplicate_events(self):
        event1 = {"target_id": "t1", "deployment_id": "d1", "state": "failing", "evidence": "error1"}
        
        # New record
        record1, state1 = notify.record_incident([], event1, self.now)
        self.assertEqual(len(state1), 1)
        self.assertEqual(record1['incident_key'], "t1:d1")
        self.assertEqual(state1[0]['evidence'], "error1")
        
        # Duplicate record updates the existing one without appending
        later = datetime(2023, 10, 20, 12, 5, 0, tzinfo=timezone.utc)
        event2 = {"target_id": "t1", "deployment_id": "d1", "state": "escalated", "evidence": "error2"}
        record2, state2 = notify.record_incident(state1, event2, later)
        
        self.assertEqual(len(state2), 1)
        self.assertEqual(record2['incident_key'], "t1:d1")
        self.assertEqual(record2['evidence'], "error2")
        self.assertEqual(record2['state'], "escalated")
        self.assertEqual(record2['updated_at'], later.isoformat())
        self.assertEqual(record2['created_at'], self.now.isoformat())
        
        # Original state1 should not be mutated
        self.assertEqual(state1[0]['evidence'], "error1")
        
    def test_delivery_outage(self):
        def failing_send(record):
            raise RuntimeError("Delivery outage")
            
        record = {"incident_key": "t2:d2"}
        notified_record = notify.notify(record, send=failing_send, attempts=2)
        
        # Check delivery status
        self.assertFalse(notified_record['notification']['delivered'])
        self.assertEqual(notified_record['notification']['attempts'], 2)
        
        # Record is returned properly, no exception is raised
        self.assertEqual(notified_record['incident_key'], "t2:d2")
        
    def test_redaction(self):
        event = {
            "token": "secret123",
            "api_key": "mykey",
            "authorization": "auth_val",
            "nested": {
                "secret": "mysecret",
                "normal": "Bearer token_val",
                "other": "ghp_12345",
                "safe": "safe_val",
                "key_with_sk": "sk-secret"
            }
        }
        
        redacted = notify.redact(event)
        
        # Check redacted keys
        self.assertEqual(redacted["token"], "[REDACTED]")
        self.assertEqual(redacted["api_key"], "[REDACTED]")
        self.assertEqual(redacted["authorization"], "[REDACTED]")
        self.assertEqual(redacted["nested"]["secret"], "[REDACTED]")
        
        # Check redacted values
        self.assertEqual(redacted["nested"]["normal"], "[REDACTED]")
        self.assertEqual(redacted["nested"]["other"], "[REDACTED]")
        self.assertEqual(redacted["nested"]["key_with_sk"], "[REDACTED]")
        
        # Check safe value remains
        self.assertEqual(redacted["nested"]["safe"], "safe_val")

        # Serialized record must never leak secrets
        blob = json.dumps(redacted)
        for leaked in ("secret123", "mykey", "auth_val", "mysecret",
                        "token_val", "ghp_12345", "sk-secret"):
            self.assertNotIn(leaked, blob)

        # Bearer-looking strings inside lists are masked too
        listed = notify.redact({"evidence": ["ok", "Bearer abc123"]})
        self.assertEqual(listed["evidence"], ["ok", "[REDACTED]"])
        
        # Ensure original event is not mutated
        self.assertEqual(event["token"], "secret123")

    def test_retry_succeeds(self):
        attempts = [0]
        def flaky_send(record):
            attempts[0] += 1
            if attempts[0] == 1:
                raise RuntimeError("Temporary failure")
                
        record = {"incident_key": "t3:d3"}
        notified_record = notify.notify(record, send=flaky_send, attempts=2)
        
        # Successfully delivered on retry
        self.assertTrue(notified_record['notification']['delivered'])
        self.assertEqual(notified_record['notification']['attempts'], 2)
        self.assertEqual(attempts[0], 2)
        
    def test_atomic_write_no_mutation(self):
        records = [{"incident_key": "t4:d4"}]
        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "state.json")
            
            # Write state
            notify.write_state(path, records)
            
            # File should exist and contents match
            self.assertTrue(os.path.exists(path))
            with open(path, 'r', encoding='utf-8') as f:
                saved = json.load(f)
            self.assertEqual(saved, records)
            
            # No .tmp file should exist
            self.assertFalse(os.path.exists(path + ".tmp"))

if __name__ == '__main__':
    unittest.main()
