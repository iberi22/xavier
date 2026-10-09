import unittest
import sys
import os

sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..', 'guardian')))

import cloudflare_pages

class TestCloudflarePages(unittest.TestCase):
    def setUp(self):
        self.valid_decision = {
            "action": "rollback",
            "deployment_id": "prod_1",
            "previous_known_good_id": "good_1",
            "to_deployment_id": "good_1"
        }
        self.valid_target = {
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
        self.assertEqual(res["result"], "planned")
        self.assertFalse(res["verified"])
        self.assertEqual(res["to_deployment_id"], "good_1")

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
        self.assertEqual(res["result"], "planned")
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

    def test_unretained_rejected(self):
        state = {"retained": {}}
        res = cloudflare_pages.plan_rollback(
            self.valid_decision,
            self.valid_target,
            state,
            dry_run=True
        )
        self.assertEqual(res["result"], "rejected")

    def test_current_mismatch_rejected(self):
        decision = dict(self.valid_decision)
        decision["deployment_id"] = "prod_2"
        res = cloudflare_pages.plan_rollback(
            decision,
            self.valid_target,
            self.valid_state,
            dry_run=True
        )
        self.assertEqual(res["result"], "rejected")

if __name__ == '__main__':
    unittest.main()
