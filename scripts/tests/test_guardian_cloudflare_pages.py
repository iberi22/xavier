import unittest
import sys
import os
import importlib.util

def load_module():
    path = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', 'guardian', 'cloudflare_pages.py'))
    spec = importlib.util.spec_from_file_location("cloudflare_pages", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

cloudflare_pages = load_module()

class TestCloudflarePages(unittest.TestCase):
    def setUp(self):
        self.valid_decision = {
            "action": "rollback",
            "deployment_id": "prod_1",
            "previous_known_good_id": "good_1"
        }
        self.valid_target = {
            "id": "target_1",
            "kind": "cf_pages",
            "production_id": "prod_1"
        }
        self.valid_state = {
            "retained": {
                "good_1": {
                    "artifact_digest": "sha256:" + "a" * 64
                }
            }
        }

    def test_dry_run_default(self):
        def api_spy(*args):
            raise Exception("api should not be called")

        res = cloudflare_pages.plan_rollback(
            self.valid_decision,
            self.valid_target,
            self.valid_state,
            api=api_spy,
            dry_run=True
        )
        self.assertEqual(res["result"], "skipped")
        self.assertFalse(res["verified"])
        self.assertEqual(res["to_deployment_id"], "good_1")
        self.assertTrue("target_id" in res)
        self.assertTrue("observed_at" in res)

    def test_apply_success(self):
        def api_spy(to_id):
            return to_id

        res = cloudflare_pages.plan_rollback(
            self.valid_decision,
            self.valid_target,
            self.valid_state,
            api=api_spy,
            dry_run=False
        )
        self.assertEqual(res["result"], "applied")
        self.assertTrue(res["verified"])
        self.assertEqual(res["to_deployment_id"], "good_1")

    def test_apply_provider_error(self):
        def api_spy(to_id):
            raise Exception("500 Internal Server Error")

        res = cloudflare_pages.plan_rollback(
            self.valid_decision,
            self.valid_target,
            self.valid_state,
            api=api_spy,
            dry_run=False
        )
        self.assertEqual(res["result"], "failed")
        self.assertFalse(res["verified"])

    def test_contract_decision_without_to_deployment_id_applies(self):
        """A real DecisionRecord carries previous_known_good_id, not to_deployment_id."""
        decision = {
            "action": "rollback",
            "deployment_id": "prod_1",
            "previous_known_good_id": "good_1",
        }
        self.assertNotIn("to_deployment_id", decision)

        def api_spy(to_id):
            return to_id

        res = cloudflare_pages.plan_rollback(
            decision,
            self.valid_target,
            self.valid_state,
            api=api_spy,
            dry_run=False
        )
        self.assertEqual(res["result"], "applied")
        self.assertTrue(res["verified"])
        self.assertEqual(res["to_deployment_id"], "good_1")

    def test_missing_previous_known_good_rejected(self):
        decision = {
            "action": "rollback",
            "deployment_id": "prod_1",
        }
        res = cloudflare_pages.plan_rollback(
            decision,
            self.valid_target,
            self.valid_state,
            dry_run=False
        )
        self.assertEqual(res["result"], "skipped")
        self.assertFalse(res["verified"])

    def test_unretained_rejected(self):
        state = {"retained": {}}
        res = cloudflare_pages.plan_rollback(
            self.valid_decision,
            self.valid_target,
            state,
            dry_run=True
        )
        self.assertEqual(res["result"], "skipped")
        self.assertFalse(res["verified"])

    def test_current_mismatch_rejected(self):
        decision = dict(self.valid_decision)
        decision["deployment_id"] = "prod_2"
        res = cloudflare_pages.plan_rollback(
            decision,
            self.valid_target,
            self.valid_state,
            dry_run=True
        )
        self.assertEqual(res["result"], "skipped")
        self.assertFalse(res["verified"])

if __name__ == '__main__':
    unittest.main()
