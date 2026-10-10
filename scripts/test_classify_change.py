import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

# Load the script dynamically for unit tests
script_path = os.path.join(os.path.dirname(__file__), "classify-change.py")
spec = importlib.util.spec_from_file_location("classify_change", script_path)
classify_change = importlib.util.module_from_spec(spec)
sys.modules["classify_change"] = classify_change
spec.loader.exec_module(classify_change)

class TestClassifyChange(unittest.TestCase):

    def test_default_to_ask_for_ambiguous_paths(self):
        result = classify_change.compute_classification(
            paths=["some_folder/file.txt"], lines=10, actor="human", label=None
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_show_path_for_src_and_crates(self):
        result = classify_change.compute_classification(
            paths=["src/main.rs", "crates/my_lib/lib.rs"], lines=20, actor="agent", label=None
        )
        self.assertEqual(result["class"], "flow:show")

    def test_agent_limit_over_files(self):
        result = classify_change.compute_classification(
            paths=["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs"], lines=10, actor="agent", label=None
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_agent_limit_over_lines(self):
        result = classify_change.compute_classification(
            paths=["src/main.rs"], lines=401, actor="agent", label=None
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_sensitive_path_crypto(self):
        result = classify_change.compute_classification(
            paths=["src/crypto/mod.rs"], lines=10, actor="agent", label=None
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_sensitive_path_cargo_toml(self):
        result = classify_change.compute_classification(
            paths=["crates/foo/Cargo.toml"], lines=10, actor="human", label=None
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_sensitive_path_docs(self):
        result = classify_change.compute_classification(
            paths=["docs/design/trunk-based/01-DESIGN.md"], lines=10, actor="human", label=None
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_sensitive_path_symlink_escape(self):
        result = classify_change.compute_classification(
            paths=["src/../Cargo.toml"], lines=10, actor="human", label=None
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_sensitive_path_env(self):
        result = classify_change.compute_classification(
            paths=[".env.example"], lines=2, actor="agent", label=None
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_diff_rename_or_delete(self):
        result = classify_change.compute_classification(
            paths=["src/old.rs", "src/new.rs"], lines=20, actor="agent", label=None, has_delete_or_rename=True
        )
        self.assertEqual(result["class"], "flow:show")

    def test_diff_rename_or_delete_ask(self):
        result = classify_change.compute_classification(
            paths=["src/old.rs", "docs/new.md"], lines=20, actor="agent", label=None, has_delete_or_rename=True
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_diff_binary_file(self):
        result = classify_change.compute_classification(
            paths=["src/asset.png"], lines=0, actor="human", label=None, has_binary=True
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_diff_empty_range(self):
        result = classify_change.compute_classification(
            paths=[], lines=0, actor="human", label=None, is_empty_diff=True
        )
        self.assertEqual(result["class"], "flow:ask")

    def test_cli_weaker_label_emits_boolean(self):
        result = classify_change.compute_classification(
            paths=["docs/README.md"], lines=10, actor="agent", label="flow:ship"
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertTrue(result["label_downgrade_attempted"])

    def test_cli_stricter_label_keeps_reason(self):
        result = classify_change.compute_classification(
            paths=["src/main.rs"], lines=10, actor="agent", label="flow:ask"
        )
        self.assertEqual(result["class"], "flow:ask")
        self.assertFalse(result["label_downgrade_attempted"])


class TestClassifyChangeCLIIntegration(unittest.TestCase):
    def setUp(self):
        self.test_dir = tempfile.TemporaryDirectory()
        self.repo_dir = self.test_dir.name

        # Init dummy git repo
        subprocess.run(["git", "init"], cwd=self.repo_dir, check=True, capture_output=True)
        subprocess.run(["git", "config", "user.name", "Test"], cwd=self.repo_dir, check=True)
        subprocess.run(["git", "config", "user.email", "test@test.com"], cwd=self.repo_dir, check=True)

        # Initial commit
        subprocess.run(["git", "commit", "--allow-empty", "-m", "initial"], cwd=self.repo_dir, check=True)

    def tearDown(self):
        self.test_dir.cleanup()

    def run_cli(self, *args):
        # We call the script from the original path but execute it within the dummy repo
        script_exec = os.path.abspath(script_path)
        res = subprocess.run([sys.executable, script_exec] + list(args), cwd=self.repo_dir, capture_output=True, text=True)
        return json.loads(res.stdout.strip())

    def test_cli_both_modes_is_ask(self):
        # Providing both --base/--head and --paths/--lines should default to Ask
        res = self.run_cli("--base", "HEAD~1", "--head", "HEAD", "--paths", "src/main.rs", "--lines", "10")
        self.assertEqual(res["class"], "flow:ask")
        self.assertIn("Empty diff, missing range, or zero paths defaults to Ask.", res["reason"])

    def test_cli_three_dot_empty(self):
        # base...head is empty
        # create a branch, no new commits
        subprocess.run(["git", "checkout", "-b", "feature"], cwd=self.repo_dir, check=True)
        res = self.run_cli("--base", "main", "--head", "feature")
        self.assertEqual(res["class"], "flow:ask")
        self.assertIn("Empty diff", res["reason"])

    def test_cli_binary_dash_dash(self):
        # Add a binary file (dummy binary)
        bin_path = os.path.join(self.repo_dir, "src", "dummy.bin")
        os.makedirs(os.path.dirname(bin_path), exist_ok=True)
        with open(bin_path, "wb") as f:
            f.write(b'\x00\x01\x02\x03')
        subprocess.run(["git", "add", "src/dummy.bin"], cwd=self.repo_dir, check=True)
        subprocess.run(["git", "commit", "-m", "add binary"], cwd=self.repo_dir, check=True)

        res = self.run_cli("--base", "HEAD~1", "--head", "HEAD")
        self.assertEqual(res["class"], "flow:ask")
        self.assertIn("binary", res["reason"])

    def test_cli_rename_numstat(self):
        # Add a file
        text_path = os.path.join(self.repo_dir, "src", "dummy.txt")
        os.makedirs(os.path.dirname(text_path), exist_ok=True)
        with open(text_path, "w") as f:
            f.write("hello\nworld\n")
        subprocess.run(["git", "add", "src/dummy.txt"], cwd=self.repo_dir, check=True)
        subprocess.run(["git", "commit", "-m", "add text"], cwd=self.repo_dir, check=True)

        # Rename it
        new_text_path = os.path.join(self.repo_dir, "src", "dummy2.txt")
        subprocess.run(["git", "mv", "src/dummy.txt", "src/dummy2.txt"], cwd=self.repo_dir, check=True)
        subprocess.run(["git", "commit", "-m", "rename text"], cwd=self.repo_dir, check=True)

        res = self.run_cli("--base", "HEAD~1", "--head", "HEAD")
        # Since it's in src/, rename is allowed -> Show
        self.assertEqual(res["class"], "flow:show")

if __name__ == '__main__':
    unittest.main()
