import unittest
import importlib.util
import tempfile
import json
import os

# "unittest, importlib.util.spec_from_file_location pattern from scripts/tests/test_guardian_targets.py"
spec = importlib.util.spec_from_file_location("release_incident", "scripts/guardian/release_incident.py")
release_incident = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_incident)

class TestReleaseIncident(unittest.TestCase):
    def test_migration_signal(self):
        event = {"kind": "source_revert", "migration_signal": True}
        decision = {
            "attribution": {
                "confident": True,
                "commits": ["abcdef"],
                "reversible": True
            }
        }
        res = release_incident.classify(event, decision)
        self.assertEqual(res["state"], "escalated")
        self.assertIsNone(res["revert_plan"])
        self.assertFalse(res["tag_moved"])

    def test_security_signal(self):
        event = {"kind": "source_revert"}
        decision = {
            "security_signal": True,
            "attribution": {
                "confident": True,
                "commits": ["abcdef"],
                "reversible": True
            }
        }
        res = release_incident.classify(event, decision)
        self.assertEqual(res["state"], "escalated")
        self.assertIsNone(res["revert_plan"])
        self.assertFalse(res["tag_moved"])

    def test_confident_single_reversible_commit(self):
        event = {"kind": "source_revert"}
        decision = {
            "attribution": {
                "confident": True,
                "commits": ["abcdef"],
                "reversible": True
            }
        }
        res = release_incident.classify(event, decision)
        self.assertEqual(res["state"], "revert_planned")
        self.assertIsNotNone(res["revert_plan"])
        self.assertEqual(res["revert_plan"]["commit"], "abcdef")
        self.assertFalse(res["revert_plan"]["panel_approved"])
        self.assertFalse(res["tag_moved"])

    def test_uncertain_attribution(self):
        event = {"kind": "source_revert"}
        decision = {
            "attribution": {
                "confident": False,
                "commits": ["abcdef"],
                "reversible": True
            }
        }
        res = release_incident.classify(event, decision)
        self.assertEqual(res["state"], "escalated")
        self.assertIsNone(res["revert_plan"])

    def test_multi_commit(self):
        event = {"kind": "source_revert"}
        decision = {
            "attribution": {
                "confident": True,
                "commits": ["abcdef", "123456"],
                "reversible": True
            }
        }
        res = release_incident.classify(event, decision)
        self.assertEqual(res["state"], "escalated")
        self.assertIsNone(res["revert_plan"])

    def test_tag_event(self):
        event = {"kind": "release_tag", "tag": "v1.2.3"}
        decision = {"evidence": {"previous_known_good_id": "v1.2.2"}}
        res = release_incident.classify(event, decision)
        self.assertEqual(res["kind"], "release_tag")
        self.assertFalse(res["tag_moved"])
        self.assertEqual(res["recommended_version"], "v1.2.2")
        self.assertEqual(res["state"], "escalated")
        self.assertIsNone(res["revert_plan"])

    def test_input_immutability(self):
        event = {"kind": "source_revert"}
        decision = {
            "attribution": {
                "confident": True,
                "commits": ["abcdef"],
                "reversible": True
            }
        }

        event_copy = dict(event)
        decision_copy = dict(decision)
        decision_copy["attribution"] = dict(decision["attribution"])

        release_incident.classify(event, decision)

        self.assertEqual(event, event_copy)
        self.assertEqual(decision, decision_copy)

if __name__ == "__main__":
    unittest.main()
