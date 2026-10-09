import unittest
import importlib.util
import sys
import os

# Dynamically import the script since it's not a package
spec = importlib.util.spec_from_file_location("cloudflare_worker", os.path.join(os.path.dirname(__file__), "..", "guardian", "cloudflare_worker.py"))
cfw = importlib.util.module_from_spec(spec)
sys.modules["cloudflare_worker"] = cfw
spec.loader.exec_module(cfw)

class TestCloudflareWorkerRecovery(unittest.TestCase):
    def setUp(self):
        self.decision = {
            "action": "rollback",
            "deployment_id": "prod-123",
            "version_id": "ver-abc"
        }
        self.target = {
            "kind": "cf_worker",
            "production_id": "prod-123"
        }
        self.state = {
            "retained_versions": {
                "ver-abc": {"traffic": 100}
            }
        }

    def test_wrong_kind(self):
        self.target["kind"] = "cf_pages"
        res = cfw.plan_rollback(self.decision, self.target, self.state)
        self.assertEqual(res["result"], "rejected")

    def test_wrong_action(self):
        self.decision["action"] = "deploy"
        res = cfw.plan_rollback(self.decision, self.target, self.state)
        self.assertEqual(res["result"], "rejected")

    def test_wrong_deployment_id(self):
        self.decision["deployment_id"] = "prod-456"
        res = cfw.plan_rollback(self.decision, self.target, self.state)
        self.assertEqual(res["result"], "rejected")

    def test_unknown_version(self):
        self.state["retained_versions"] = {}
        res = cfw.plan_rollback(self.decision, self.target, self.state)
        self.assertEqual(res["result"], "rejected")
        self.assertIn("absent", res["detail"].lower())

    def test_traffic_mismatch(self):
        self.state["retained_versions"]["ver-abc"]["traffic"] = 90
        res = cfw.plan_rollback(self.decision, self.target, self.state)
        self.assertEqual(res["result"], "rejected")
        self.assertIn("traffic percentage", res["detail"].lower())

    def test_dry_run_no_api_call(self):
        api_calls = []
        def mock_api(action, ver):
            api_calls.append((action, ver))
            return {"version_id": ver, "traffic": 100}

        res = cfw.plan_rollback(self.decision, self.target, self.state, api=mock_api, dry_run=True)
        self.assertEqual(res["result"], "planned")
        self.assertFalse(res["verified"])
        self.assertEqual(len(api_calls), 0)

    def test_apply_verifies_version(self):
        api_calls = []
        def mock_api(action, ver):
            api_calls.append((action, ver))
            if action == "probe":
                return {"version_id": ver, "traffic": 100}
            return {}

        res = cfw.plan_rollback(self.decision, self.target, self.state, api=mock_api, dry_run=False)
        self.assertEqual(res["result"], "verified")
        self.assertTrue(res["verified"])
        self.assertEqual(len(api_calls), 2)

    def test_apply_fails_on_traffic_mismatch(self):
        def mock_api(action, ver):
            if action == "probe":
                return {"version_id": ver, "traffic": 50}
            return {}

        res = cfw.plan_rollback(self.decision, self.target, self.state, api=mock_api, dry_run=False)
        self.assertEqual(res["result"], "failed")
        self.assertFalse(res["verified"])

    def test_apply_fails_on_wrong_version(self):
        def mock_api(action, ver):
            if action == "probe":
                return {"version_id": "ver-wrong", "traffic": 100}
            return {}

        res = cfw.plan_rollback(self.decision, self.target, self.state, api=mock_api, dry_run=False)
        self.assertEqual(res["result"], "failed")
        self.assertFalse(res["verified"])

    def test_apply_handles_provider_error(self):
        def mock_api(action, ver):
            raise Exception("Provider error with secretToken123!")

        res = cfw.plan_rollback(self.decision, self.target, self.state, api=mock_api, dry_run=False)
        self.assertEqual(res["result"], "failed")
        self.assertFalse(res["verified"])
        self.assertNotIn("secretToken123", res["detail"])
        self.assertEqual(res["detail"], "provider error (redacted)")

if __name__ == "__main__":
    unittest.main()
