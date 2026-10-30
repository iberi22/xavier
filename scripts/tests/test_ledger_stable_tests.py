import unittest
import os
import sys

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))
from check_stable_tests import verify_stable_tests

class TestCheckStableTests(unittest.TestCase):
    def test_declared_and_run_pass(self):
        features_data = {
            "features": [
                {
                    "id": "feat-1",
                    "status": "stable",
                    "tests": ["test_foo"]
                }
            ]
        }
        log_text = "  ▶ test_foo\n    ✅ --test something (passed=1)\n"
        errors = verify_stable_tests(features_data, log_text)
        self.assertEqual(len(errors), 0)

    def test_declared_missing_fail(self):
        features_data = {
            "features": [
                {
                    "id": "feat-2",
                    "status": "stable",
                    "tests": ["test_bar"]
                }
            ]
        }
        log_text = "  ▶ test_other\n    ✅ --test something (passed=1)\n"
        errors = verify_stable_tests(features_data, log_text)
        self.assertEqual(len(errors), 1)
        self.assertIn("feat-2", errors[0])
        self.assertIn("test_bar", errors[0])

if __name__ == "__main__":
    unittest.main()
