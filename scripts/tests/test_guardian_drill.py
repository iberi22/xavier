import unittest
import importlib.util
import os
from datetime import datetime, timezone, timedelta

spec_drill = importlib.util.spec_from_file_location(
    "drill", os.path.join(os.path.dirname(__file__), "..", "guardian", "drill.py")
)
drill = importlib.util.module_from_spec(spec_drill)
spec_drill.loader.exec_module(drill)

spec_metrics = importlib.util.spec_from_file_location(
    "metrics", os.path.join(os.path.dirname(__file__), "..", "guardian", "metrics.py")
)
metrics = importlib.util.module_from_spec(spec_metrics)
spec_metrics.loader.exec_module(metrics)

class TestGuardianDrill(unittest.TestCase):
    def setUp(self):
        self.now = datetime(2023, 10, 20, 12, 0, 0, tzinfo=timezone.utc)
        self.target = {"id": "target1"}

    def test_failed_smoke(self):
        adapters = {
            "probe": lambda s: True,
            "decide": lambda s: True,
            "act": lambda s: True,
            "notify": lambda: None
        }
        res = drill.run_drill(self.target, "failed_smoke", adapters, self.now)
        self.assertEqual(res["target_id"], "target1")
        self.assertEqual(res["scenario"], "failed_smoke")
        self.assertEqual(res["outcome"], "restored")
        self.assertFalse(res["escalated"])
        self.assertFalse(res["false_positive"])

    def test_sustained_error(self):
        adapters = {
            "probe": lambda s: True,
            "decide": lambda s: True,
            "act": lambda s: True,
            "notify": lambda: None
        }
        res = drill.run_drill(self.target, "sustained_error", adapters, self.now)
        self.assertEqual(res["outcome"], "restored")
        self.assertFalse(res["escalated"])
        self.assertFalse(res["false_positive"])

    def test_false_alarm(self):
        adapters = {
            "probe": lambda s: False,
            "decide": lambda s: True,
            "act": lambda s: True,
            "notify": lambda: None
        }
        res = drill.run_drill(self.target, "false_alarm", adapters, self.now)
        self.assertEqual(res["outcome"], "restored")
        self.assertFalse(res["escalated"])
        self.assertTrue(res["false_positive"])

    def test_missing_known_good(self):
        adapters = {
            "probe": lambda s: True,
            "decide": lambda s: False,
            "act": lambda s: True,
            "notify": lambda: None
        }
        res = drill.run_drill(self.target, "missing_known_good", adapters, self.now)
        self.assertEqual(res["outcome"], "escalated")
        self.assertTrue(res["escalated"])
        self.assertFalse(res["false_positive"])
        self.assertIsNone(res["restored_at"])

    def test_rollback_failure(self):
        adapters = {
            "probe": lambda s: True,
            "decide": lambda s: True,
            "act": lambda s: False,
            "notify": lambda: None
        }
        res = drill.run_drill(self.target, "rollback_failure", adapters, self.now)
        self.assertEqual(res["outcome"], "failed")
        self.assertTrue(res["escalated"])
        self.assertFalse(res["false_positive"])
        self.assertIsNone(res["restored_at"])

    def test_guardian_outage(self):
        res = drill.run_drill(self.target, "guardian_outage", {}, self.now)
        self.assertEqual(res["outcome"], "failed")
        self.assertTrue(res["escalated"])
        self.assertFalse(res["false_positive"])
        self.assertIsNone(res["restored_at"])

    def test_mttr_series(self):
        results = [
            drill.run_drill(self.target, "failed_smoke", {}, self.now),
            drill.run_drill(self.target, "missing_known_good", {}, self.now),
            drill.run_drill(self.target, "false_alarm", {}, self.now),
        ]

        # Modify some times so we can test MTTR logic
        results[0]["first_bad_signal_at"] = (self.now - timedelta(seconds=5)).isoformat()
        results[0]["detected_at"] = self.now.isoformat()
        results[0]["decided_at"] = (self.now + timedelta(seconds=10)).isoformat()
        results[0]["restored_at"] = (self.now + timedelta(seconds=30)).isoformat()

        series = metrics.mttr_series(results)

        self.assertIn("target1", series)
        s = series["target1"]
        self.assertEqual(s["time_to_detect"], 10.0) # 5.0 from failed_smoke + 5.0 from missing_known_good
        self.assertEqual(s["production_mttr"], 30.0) # From failed_smoke
        self.assertEqual(s["unresolved"], 1) # From missing_known_good
        self.assertEqual(s["false_positives"], 1) # From false_alarm
        self.assertNotIn("integration_mttr", s)

    def test_drill_result_contract_keys(self):
        res = drill.run_drill(self.target, "failed_smoke", {}, self.now)
        contract = {"target_id", "scenario", "detected_at", "decided_at",
                    "restored_at", "escalated", "false_positive", "outcome"}
        self.assertTrue(contract.issubset(set(res.keys())))
        self.assertIn(res["outcome"], ("restored", "escalated", "failed"))
        self.assertTrue(res["detected_at"].endswith("Z"))
        self.assertTrue(res["restored_at"].endswith("Z"))

    def test_time_to_detect_from_producer_field(self):
        # Regression: time_to_detect must come from a real DrillResult field.
        res = drill.run_drill(self.target, "failed_smoke", {}, self.now)
        self.assertIn("first_bad_signal_at", res)
        self.assertTrue(res["first_bad_signal_at"].endswith("Z"))
        series = metrics.mttr_series([res])
        self.assertEqual(series["target1"]["time_to_detect"], 5.0)

if __name__ == "__main__":
    unittest.main()
