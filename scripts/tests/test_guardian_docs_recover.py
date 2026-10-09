import unittest
import importlib.util
import os

class TestDocsRecover(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("docs_recover", "scripts/guardian/docs_recover.py")
        self.docs_recover = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.docs_recover)

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

    def test_arbitrary_sha_rejected(self):
        decision = dict(self.valid_decision)
        decision["source_sha"] = "0000000000000000000000000000000000000000"
        res = self.docs_recover.plan_recovery(decision, self.valid_target, self.valid_retained)
        self.assertEqual(res["status"], "rejected")
        self.assertIn("arbitrary SHA", res["detail"])

    def test_missing_retained_artifact_rejected(self):
        retained = dict(self.valid_retained)
        del retained["artifact_digest"]
        res = self.docs_recover.plan_recovery(self.valid_decision, self.valid_target, retained)
        self.assertEqual(res["status"], "rejected")
        self.assertIn("missing or malformed", res["detail"])

    def test_malformed_retained_artifact_rejected(self):
        retained = dict(self.valid_retained)
        retained["artifact_digest"] = "sha256:123"
        res = self.docs_recover.plan_recovery(self.valid_decision, self.valid_target, retained)
        self.assertEqual(res["status"], "rejected")
        self.assertIn("missing or malformed", res["detail"])

    def test_non_previous_known_good_id_rejected(self):
        decision = dict(self.valid_decision)
        decision["to_deployment_id"] = "some_other_id"
        res = self.docs_recover.plan_recovery(decision, self.valid_target, self.valid_retained)
        self.assertEqual(res["status"], "rejected")
        self.assertIn("previous_known_good_id", res["detail"])

    def test_dry_run_performs_no_action(self):
        res = self.docs_recover.plan_recovery(self.valid_decision, self.valid_target, self.valid_retained, dry_run=True)
        self.assertEqual(res["status"], "planned")
        self.assertFalse(res["verified"])
        self.assertEqual(res["plan"]["to_deployment_id"], "good_id")
        self.assertEqual(res["plan"]["artifact_digest"], self.valid_retained["artifact_digest"])

    def test_apply_plan_keeps_verified_false_without_probe(self):
        res = self.docs_recover.plan_recovery(self.valid_decision, self.valid_target, self.valid_retained, dry_run=False)
        self.assertEqual(res["status"], "applied")
        self.assertFalse(res["verified"])

if __name__ == '__main__':
    unittest.main()
