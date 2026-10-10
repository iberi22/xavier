import unittest
import subprocess
import json
import os

class TestRustImpact(unittest.TestCase):
    def run_script(self, args, check=True):
        cmd = ["python3", "scripts/rust-impact.py"] + args
        result = subprocess.run(cmd, capture_output=True, text=True)
        if check and result.returncode != 0:
            print(result.stderr)
            result.check_returncode()
        return result

    def test_help(self):
        result = self.run_script(["--help"], check=False)
        self.assertEqual(result.returncode, 0)
        self.assertIn("Calculate Rust test impact", result.stdout)

    def test_cargo_toml_triggers_fallback(self):
        result = self.run_script(["--paths", "Cargo.toml"])
        output = json.loads(result.stdout)
        self.assertTrue(output["fallback"])
        self.assertIn("Modified manifest/lockfile", output["reason"])
        self.assertIn("xavier", output["packages"])

    def test_cargo_lock_triggers_fallback(self):
        result = self.run_script(["--paths", "Cargo.lock"])
        output = json.loads(result.stdout)
        self.assertTrue(output["fallback"])
        self.assertIn("Modified manifest/lockfile", output["reason"])
        self.assertIn("xavier", output["packages"])

    def test_github_ci_triggers_fallback(self):
        result = self.run_script(["--paths", ".github/workflows/ci.yml"])
        output = json.loads(result.stdout)
        self.assertTrue(output["fallback"])
        self.assertIn("Modified CI file", output["reason"])
        self.assertIn("xavier", output["packages"])

    def test_unknown_path_triggers_fallback(self):
        result = self.run_script(["--paths", "unknown/path.txt"])
        output = json.loads(result.stdout)
        self.assertTrue(output["fallback"])
        self.assertIn("Unknown path outside recognized rust package directories", output["reason"])
        self.assertIn("xavier", output["packages"])

    def test_scoped_selection_and_reverse_deps(self):
        result = self.run_script(["--paths", "crates/xavier-pageindex/src/lib.rs"])
        output = json.loads(result.stdout)
        self.assertFalse(output["fallback"])
        self.assertIn("xavier-pageindex", output["packages"])
        self.assertIn("xavier", output["packages"])

    def test_zero_tests_selected_fails(self):
        result = self.run_script(["--paths"], check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Error: Zero selected-test result fails", result.stderr)

if __name__ == '__main__':
    unittest.main()
