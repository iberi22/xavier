import unittest
import datetime
import sys
import os

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))

from guardian.decide import decide

class TestGuardianDecide(unittest.TestCase):
    def setUp(self):
        self.now = datetime.datetime.now(datetime.timezone.utc)
        self.base_evidence = {
            "deployment_id": "deploy_123",
            "correlated": True,
            "signal": "unhealthy"
        }
        self.base_inventory = {
            "targets": [
                {
                    "id": "target_1",
                    "kind": "cf_pages",
                    "production_id": "deploy_123",
                    "previous_known_good_id": "target_0"
                },
                {
                    "id": "target_0",
                    "kind": "cf_pages",
                    "production_id": "deploy_000"
                }
            ]
        }
        self.base_state = {
            "attempts": {},
            "last_action_time": (self.now - datetime.timedelta(hours=1)).isoformat()
        }

    def test_rollback_success(self):
        result = decide(self.base_evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "rollback")

    def test_threshold_uncorrelated(self):
        evidence = dict(self.base_evidence, correlated=False)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_threshold_unknown(self):
        evidence = dict(self.base_evidence, correlated=False, status="unknown", signal="unknown")
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "hold")

    def test_cooldown_active(self):
        state = dict(self.base_state, last_action_time=(self.now - datetime.timedelta(minutes=5)).isoformat())
        result = decide(self.base_evidence, self.base_inventory, state, self.now)
        self.assertEqual(result["action"], "escalate")
        self.assertIn("cooldown", result["summary"].lower() + result["reason"].lower())

    def test_one_attempt_per_deployment(self):
        state = dict(self.base_state, attempts={"deploy_123": 1})
        result = decide(self.base_evidence, self.base_inventory, state, self.now)
        self.assertEqual(result["action"], "escalate")
        self.assertIn("attempts", result["summary"].lower() + result["reason"].lower())

    def test_escalation_migration(self):
        evidence = dict(self.base_evidence, migration_flag=True)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_security(self):
        evidence = dict(self.base_evidence, security_flag=True)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_uncertain_attribution(self):
        evidence = dict(self.base_evidence, unknown_attribution=True)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_missing_target(self):
        inventory = {"targets": []}
        result = decide(self.base_evidence, inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_missing_previous_target(self):
        inventory = {
            "targets": [
                {
                    "id": "target_1",
                    "kind": "cf_pages",
                    "production_id": "deploy_123",
                    "previous_known_good_id": "target_999"
                }
            ]
        }
        result = decide(self.base_evidence, inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_immutability(self):
        import copy
        evidence_copy = copy.deepcopy(self.base_evidence)
        inventory_copy = copy.deepcopy(self.base_inventory)
        state_copy = copy.deepcopy(self.base_state)

        decide(self.base_evidence, self.base_inventory, self.base_state, self.now)

        self.assertEqual(self.base_evidence, evidence_copy)
        self.assertEqual(self.base_inventory, inventory_copy)
        self.assertEqual(self.base_state, state_copy)

    def test_irreversible_kind(self):
        inventory = {
            "targets": [
                {
                    "id": "target_1",
                    "kind": "local_binary",
                    "production_id": "deploy_123",
                    "previous_known_good_id": "target_0"
                },
                {
                    "id": "target_0",
                    "kind": "local_binary",
                    "production_id": "deploy_000"
                }
            ]
        }
        result = decide(self.base_evidence, inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_bad_schema(self):
        result = decide([], {}, {}, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_missing_state_file(self):
        # We test that passing None for state (which happens when state file is unreadable)
        # leads to a specific error handling or escalation. In decide() this triggers ValueError,
        # which is caught and returned as an escalate action.
        result = decide(self.base_evidence, self.base_inventory, None, self.now)
        self.assertEqual(result["action"], "escalate")
        self.assertIn("parse error", result["summary"].lower())

    def test_escalation_unknown_evidence_fields(self):
        evidence = dict(self.base_evidence, wtf_field="what is this")
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")
        self.assertIn("unknown fields", result["reason"].lower())

    def test_escalation_schema_flag(self):
        evidence = dict(self.base_evidence, schema_flag=True)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_unhealthy_previous_target(self):
        inventory = {
            "targets": [
                {
                    "id": "target_1",
                    "kind": "cf_pages",
                    "production_id": "deploy_123",
                    "previous_known_good_id": "target_0"
                },
                {
                    "id": "target_0",
                    "kind": "cf_pages",
                    "production_id": "deploy_000",
                    "health_status": "unhealthy"
                }
            ]
        }
        result = decide(self.base_evidence, inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")
        self.assertIn("unhealthy", result["reason"].lower())

if __name__ == '__main__':
    unittest.main()
