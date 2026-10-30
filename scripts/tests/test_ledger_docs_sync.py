import json
import os
import subprocess
import sys
import tempfile
import unittest

class TestLedgerDocsSync(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.features_path = os.path.join(self.temp_dir.name, "features.json")
        self.doc_path = os.path.join(self.temp_dir.name, "FEATURE_STATUS.md")
        self.script_path = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "ledger-docs-sync.py"))

    def tearDown(self):
        self.temp_dir.cleanup()

    def run_script(self, args):
        cmd = [sys.executable, self.script_path, "--features", self.features_path, "--doc", self.doc_path] + args
        return subprocess.run(cmd, capture_output=True, text=True)

    def write_features(self, features_dict):
        data = {"features": features_dict}
        with open(self.features_path, "w", encoding="utf-8") as f:
            json.dump(data, f)

    def write_doc(self, content):
        with open(self.doc_path, "w", encoding="utf-8") as f:
            f.write(content)

    def read_doc(self):
        with open(self.doc_path, "r", encoding="utf-8") as f:
            return f.read()

    def test_sync_and_check(self):
        # 4 statuses, 2 categories, one without category
        features = {
            "f1": {"category": "cat1", "status": "stable"},
            "f2": {"category": "cat1", "status": "beta"},
            "f3": {"category": "cat2", "status": "in_progress"},
            "f4": {"category": "cat2", "status": "planned"},
            "f5": {"status": "stable"} # missing category -> uncategorized
        }
        self.write_features(features)

        initial_doc = "Prefix\n<!-- ledger:summary:start -->\n<!-- ledger:summary:end -->\nSuffix"
        self.write_doc(initial_doc)

        # Check stale
        res = self.run_script(["--check"])
        self.assertEqual(res.returncode, 1)
        self.assertIn("Doc is stale.", res.stderr)

        # Write
        res = self.run_script(["--write"])
        self.assertEqual(res.returncode, 0)

        doc_content = self.read_doc()

        # Check outside markers is unchanged
        self.assertTrue(doc_content.startswith("Prefix\n<!-- ledger:summary:start -->\n"))
        self.assertTrue(doc_content.endswith("\n<!-- ledger:summary:end -->\nSuffix"))

        # Check percentage (2 stable / 5 total = 40.0%)
        self.assertIn("2 stable", doc_content)
        self.assertIn("40.0% stable", doc_content)

        # Check table
        self.assertIn("| cat1 | 1 | 1 | 0 | 0 | 2 |", doc_content)
        self.assertIn("| cat2 | 0 | 0 | 1 | 1 | 2 |", doc_content)
        self.assertIn("| uncategorized | 1 | 0 | 0 | 0 | 1 |", doc_content)

        # Check fresh
        res = self.run_script(["--check"])
        self.assertEqual(res.returncode, 0)

        # Write again (idempotence)
        res = self.run_script(["--write"])
        self.assertEqual(res.returncode, 0)
        doc_content_2 = self.read_doc()
        self.assertEqual(doc_content, doc_content_2)

    def test_missing_markers(self):
        features = {"f1": {"category": "cat1", "status": "stable"}}
        self.write_features(features)

        self.write_doc("No markers here")

        res = self.run_script(["--write"])
        self.assertEqual(res.returncode, 2)

        res = self.run_script(["--check"])
        self.assertEqual(res.returncode, 2)

    def test_other_status(self):
        features = {
            "f1": {"category": "cat1", "status": "stable"},
            "f2": {"category": "cat1", "status": "weird_status"}
        }
        self.write_features(features)
        self.write_doc("<!-- ledger:summary:start -->\n<!-- ledger:summary:end -->")

        res = self.run_script(["--write"])
        self.assertEqual(res.returncode, 0)

        doc_content = self.read_doc()
        self.assertIn("| other |", doc_content)
        self.assertIn("| cat1 | 1 | 0 | 0 | 0 | 1 | 2 |", doc_content)

if __name__ == "__main__":
    unittest.main()
