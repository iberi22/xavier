import unittest
import importlib.util
import os
import copy
import datetime

spec = importlib.util.spec_from_file_location(
    "decide", os.path.join(os.path.dirname(__file__), "..", "guardian", "decide.py")
)
decide_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(decide_mod)
decide = decide_mod.decide

DECISION_KEYS = {"target_id", "kind", "deployment_id", "decided_at", "action",
                 "reason", "reversible", "previous_known_good_id", "attempts_used",
                 "cooldown_until", "evidence"}

class TestGuardianDecide(unittest.TestCase):
    def setUp(self):
        self.now = datetime.datetime(2023, 10, 20, 12, 0, 0, tzinfo=datetime.timezone.utc)
        # Contract-shaped FailureEvidence (PLAN.md §Interface contracts).
        self.base_evidence = {
            "target_id": "target_1",
            "kind": "cf_pages",
            "deployment_id": "deploy_123",
            "detected_at": "2023-10-20T11:59:00Z",
            "failed_probes": ["gha", "local"],
            "window_seconds": 300,
            "vantages": ["gha", "local"],
            "correlated": True,
            "missing_metrics": [],
            "summary": "correlated failure on deploy_123",
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

    def assertDecisionRecord(self, result):
        self.assertEqual(set(result.keys()), DECISION_KEYS)
        self.assertIn(result["action"], ("rollback", "escalate", "hold"))
        self.assertIsInstance(result["attempts_used"], int)
        self.assertIsInstance(result["evidence"], dict)
        if result["decided_at"] is not None:
            self.assertTrue(result["decided_at"].endswith("Z"))

    def test_contract_evidence_rollback(self):
        # Regression: a contract-shaped FailureEvidence must be accepted, not
        # rejected as "Unknown fields", and must yield a full DecisionRecord.
        result = decide(self.base_evidence, self.base_inventory, self.base_state, self.now)
        self.assertDecisionRecord(result)
        self.assertEqual(result["action"], "rollback")
        self.assertTrue(result["reversible"])
        self.assertEqual(result["previous_known_good_id"], "target_0")
        self.assertEqual(result["attempts_used"], 0)

    def test_threshold_uncorrelated(self):
        evidence = dict(self.base_evidence, correlated=False)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertDecisionRecord(result)
        self.assertEqual(result["action"], "escalate")

    def test_threshold_unknown_holds(self):
        evidence = dict(self.base_evidence, correlated=False, status="unknown",
                        failed_probes=[], missing_metrics=["gha"])
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertDecisionRecord(result)
        self.assertEqual(result["action"], "hold")

    def test_cooldown_active(self):
        state = dict(self.base_state, last_action_time=(self.now - datetime.timedelta(minutes=5)).isoformat())
        result = decide(self.base_evidence, self.base_inventory, state, self.now)
        self.assertDecisionRecord(result)
        self.assertEqual(result["action"], "escalate")
        self.assertIn("cooldown", result["reason"].lower())
        self.assertIsNotNone(result["cooldown_until"])

    def test_one_attempt_per_deployment(self):
        state = dict(self.base_state, attempts={"deploy_123": 1})
        result = decide(self.base_evidence, self.base_inventory, state, self.now)
        self.assertDecisionRecord(result)
        self.assertEqual(result["action"], "escalate")
        self.assertEqual(result["attempts_used"], 1)
        self.assertIn("attempts", result["reason"].lower())

    def test_escalation_migration(self):
        evidence = dict(self.base_evidence, migration_flag=True)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertDecisionRecord(result)
        self.assertEqual(result["action"], "escalate")
        self.assertFalse(result["reversible"])

    def test_escalation_security(self):
        evidence = dict(self.base_evidence, security_flag=True)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_schema_flag(self):
        evidence = dict(self.base_evidence, schema_flag=True)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_uncertain_attribution(self):
        evidence = dict(self.base_evidence, unknown_attribution=True)
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_missing_target(self):
        inventory = {"targets": []}
        result = decide(self.base_evidence, inventory, self.base_state, self.now)
        self.assertDecisionRecord(result)
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
        self.assertDecisionRecord(result)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_missing_state_file(self):
        result = decide(self.base_evidence, self.base_inventory, None, self.now)
        self.assertDecisionRecord(result)
        self.assertEqual(result["action"], "escalate")

    def test_escalation_unknown_evidence_fields(self):
        evidence = dict(self.base_evidence, wtf_field="what is this")
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")
        self.assertIn("unknown fields", result["reason"].lower())

    def test_escalation_missing_evidence_fields(self):
        evidence = dict(self.base_evidence)
        del evidence["detected_at"]
        result = decide(evidence, self.base_inventory, self.base_state, self.now)
        self.assertEqual(result["action"], "escalate")
        self.assertIn("missing evidence fields", result["reason"].lower())

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
