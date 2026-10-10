import importlib.util
import os
import sys
import tempfile
import unittest

# Load the script dynamically as instructed
script_path = os.path.join(os.path.dirname(__file__), "classify-change.py")
spec = importlib.util.spec_from_file_location("classify_change", script_path)
classify_change = importlib.util.module_from_spec(spec)
sys.modules["classify_change"] = classify_change
spec.loader.exec_module(classify_change)

class TestClassifyChange(unittest.TestCase):

    def test_agent_ship_limit_within_bounds(self):
        result = classify_change.compute_classification(
            paths=["src/main.rs", "src/lib.rs"],
            lines=200,
            actor="agent",
            label=None
        )
        self.assertEqual(result["class"], "flow:ship")

    def test_agent_ship_limit_exceeds_files(self):
        result = classify_change.compute_classification(
            paths=["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs"],
            lines=10,
            actor="agent",
            label=None
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertIn("exceeds file limit", result["reason"])

    def test_agent_ship_limit_exceeds_lines(self):
        result = classify_change.compute_classification(
            paths=["src/main.rs"],
            lines=401,
            actor="agent",
            label=None
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertIn("exceeds line limit", result["reason"])

    def test_non_agent_no_limits(self):
        result = classify_change.compute_classification(
            paths=["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs"],
            lines=1000,
            actor="human",
            label=None
        )
        self.assertEqual(result["class"], "flow:ship")

    def test_sensitive_path_workflows(self):
        result = classify_change.compute_classification(
            paths=[".github/workflows/ci.yml"],
            lines=10,
            actor="human",
            label=None
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertIn("Sensitive path", result["reason"])

    def test_sensitive_path_gitcore(self):
        result = classify_change.compute_classification(
            paths=[".gitcore/features.json"],
            lines=1,
            actor="human",
            label=None
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertIn("Sensitive path", result["reason"])

    def test_unknown_path(self):
        result = classify_change.compute_classification(
            paths=["weird_dir/file.txt"],
            lines=10,
            actor="human",
            label=None
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertIn("Unknown path", result["reason"])

    def test_label_downgrade_prevention(self):
        # Computed would be flow:ask because of lines limit, but label says flow:ship
        # Output should be flow:ask, preventing downgrade
        result = classify_change.compute_classification(
            paths=["src/main.rs"],
            lines=500,
            actor="agent",
            label="flow:ship"
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertIn("Blocked label downgrade", result["reason"])

    def test_label_upgrade_retention(self):
        # Computed would be flow:ship, but label says flow:ask
        # Output should be flow:ask, retaining restrictive label
        result = classify_change.compute_classification(
            paths=["src/main.rs"],
            lines=10,
            actor="human",
            label="flow:ask"
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertIn("Maintained existing restrictive label", result["reason"])

if __name__ == '__main__':
    unittest.main()
