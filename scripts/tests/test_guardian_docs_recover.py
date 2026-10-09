import unittest
import importlib.util
import os

ACTION_RESULT_KEYS = {"target_id", "kind", "action", "dry_run", "from_deployment_id",
                      "to_deployment_id", "result", "verified", "observed_at", "detail"}

spec = importlib.util.spec_from_file_location(
    "docs_recover",
    os.path.join(os.path.dirname(__file__), "..", "guardian", "docs_recover.py"),
)
docs_recover = importlib.util.module_from_spec(spec)
spec.loader.exec_module(docs_recover)

class TestDocsRecover(unittest.TestCase):
    def setUp(self):
        self.valid_decision = {
            "action": "rollback",
            "target_id": "docs_prod",
            "to_deployment_id": "good_id"
        }
        self.valid_target = {
            "kind": "gh_pages",
            "id": "docs_prod",
            "previous_known_good_id": "good_id"
        }
        self.valid_retained = {
            "source_sha": "1234567890123456789012345678901234567890",
            "artifact_digest": "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        }

    def assertActionResult(self, res):
        self.assertEqual(set(res.keys()), ACTION_RESULT_KEYS | {"plan"})
        self.assertEqual(res["action"], "rollback")
        self.assertEqual(res["kind"], "gh_pages")
        self.assertIn(res["result"], ("applied", "skipped", "failed"))
        self.assertIsInstance(res["dry_run"], bool)
        self.assertIsInstance(res["verified"], bool)
        self.assertTrue(res["observed_at"].endswith("Z"))

    def test_arbitrary_sha_rejected(self):
        decision = dict(self.valid_decision)
        decision["source_sha"] = "0000000000000000000000000000000000000000"
        res = docs_recover.plan_recovery(decision, self.valid_target, self.valid_retained)
        self.assertActionResult(res)
        self.assertEqual(res["result"], "skipped")
        self.assertIn("arbitrary SHA", res["detail"])

    def test_missing_retained_artifact_rejected(self):
        retained = dict(self.valid_retained)
        del retained["artifact_digest"]
        res = docs_recover.plan_recovery(self.valid_decision, self.valid_target, retained)
        self.assertEqual(res["result"], "skipped")
        self.assertIn("missing or malformed", res["detail"])

    def test_malformed_retained_artifact_rejected(self):
        retained = dict(self.valid_retained)
        retained["artifact_digest"] = "sha256:123"
        res = docs_recover.plan_recovery(self.valid_decision, self.valid_target, retained)
        self.assertEqual(res["result"], "skipped")
        self.assertIn("missing or malformed", res["detail"])

    def test_non_previous_known_good_id_rejected(self):
        decision = dict(self.valid_decision)
        decision["to_deployment_id"] = "some_other_id"
        res = docs_recover.plan_recovery(decision, self.valid_target, self.valid_retained)
        self.assertEqual(res["result"], "skipped")
        self.assertIn("previous_known_good_id", res["detail"])

    def test_dry_run_performs_no_action(self):
        res = docs_recover.plan_recovery(self.valid_decision, self.valid_target, self.valid_retained, dry_run=True)
        self.assertActionResult(res)
        self.assertEqual(res["result"], "skipped")
        self.assertFalse(res["verified"])
        self.assertEqual(res["plan"]["to_deployment_id"], "good_id")
        self.assertEqual(res["plan"]["artifact_digest"], self.valid_retained["artifact_digest"])

    def test_apply_plan_keeps_verified_false_without_probe(self):
        res = docs_recover.plan_recovery(self.valid_decision, self.valid_target, self.valid_retained, dry_run=False)
        self.assertActionResult(res)
        self.assertEqual(res["result"], "applied")
        self.assertFalse(res["verified"])

    def test_apply_with_probe_verifies(self):
        res = docs_recover.plan_recovery(self.valid_decision, self.valid_target, self.valid_retained,
                                         dry_run=False, probe=lambda plan: True)
        self.assertEqual(res["result"], "applied")
        self.assertTrue(res["verified"])

    def test_apply_with_failing_probe(self):
        res = docs_recover.plan_recovery(self.valid_decision, self.valid_target, self.valid_retained,
                                         dry_run=False, probe=lambda plan: False)
        self.assertEqual(res["result"], "failed")
        self.assertFalse(res["verified"])

    def test_contract_decision_record_is_accepted(self):
        # Regression: a contract-shaped DecisionRecord carries previous_known_good_id,
        # not to_deployment_id, and the output must be a full ActionResult.
        decision = {
            "target_id": "docs_prod",
            "kind": "gh_pages",
            "deployment_id": "current_id",
            "decided_at": "2023-10-20T12:00:00Z",
            "action": "rollback",
            "reason": "correlated failure",
            "reversible": True,
            "previous_known_good_id": "good_id",
            "attempts_used": 0,
            "cooldown_until": None,
            "evidence": {},
        }
        res = docs_recover.plan_recovery(decision, self.valid_target, self.valid_retained, dry_run=True)
        self.assertActionResult(res)
        self.assertEqual(res["result"], "skipped")
        self.assertEqual(res["to_deployment_id"], "good_id")
        self.assertEqual(res["plan"]["source_sha"], self.valid_retained["source_sha"])

if __name__ == '__main__':
    unittest.main()
