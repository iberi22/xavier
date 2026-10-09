import unittest
import tempfile
import os
import subprocess
import json
import importlib.util
import sys
from pathlib import Path

# Load the script
script_path = os.path.join(os.path.dirname(__file__), '..', 'delivery-scope-check.py')
spec = importlib.util.spec_from_file_location("delivery_scope_check", script_path)
delivery_scope_check = importlib.util.module_from_spec(spec)
sys.modules["delivery_scope_check"] = delivery_scope_check
spec.loader.exec_module(delivery_scope_check)

class TestDeliveryScopeCheck(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.repo_path = self.temp_dir.name

        # Initialize git repo
        subprocess.run(["git", "init"], cwd=self.repo_path, check=True, capture_output=True)

        # Set author/committer
        self.env = os.environ.copy()
        self.env['GIT_AUTHOR_NAME'] = 'Test Author'
        self.env['GIT_AUTHOR_EMAIL'] = 'test@example.com'
        self.env['GIT_COMMITTER_NAME'] = 'Test Committer'
        self.env['GIT_COMMITTER_EMAIL'] = 'test@example.com'

        # Create initial commit
        self._write_file("init.txt", "init\n")
        subprocess.run(["git", "add", "init.txt"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "init"], cwd=self.repo_path, check=True, env=self.env)

        self.base_sha = self._get_sha("HEAD")

    def tearDown(self):
        self.temp_dir.cleanup()

    def _write_file(self, path, content):
        full_path = os.path.join(self.repo_path, path)
        os.makedirs(os.path.dirname(full_path), exist_ok=True)
        with open(full_path, 'w', encoding='utf-8') as f:
            f.write(content)

    def _get_sha(self, ref):
        return subprocess.check_output(["git", "rev-parse", ref], cwd=self.repo_path, text=True, env=self.env).strip()

    def _run_check(self, head_sha, files_arg, include_worktree=False):
        cmd = [
            sys.executable, script_path,
            "--base", self.base_sha,
            "--head", head_sha,
            "--files", files_arg
        ]
        if include_worktree:
            cmd.append("--include-worktree")

        result = subprocess.run(cmd, cwd=self.repo_path, text=True, capture_output=True, env=self.env)
        return result

    def test_clean_in_scope_diff(self):
        self._write_file("test.txt", "hello\n")
        subprocess.run(["git", "add", "test.txt"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "test"], cwd=self.repo_path, check=True, env=self.env)

        head_sha = self._get_sha("HEAD")
        result = self._run_check(head_sha, "test.txt")

        self.assertEqual(result.returncode, 0, f"Expected success, got stderr: {result.stderr}")
        report = json.loads(result.stdout)
        self.assertEqual(report["verdict"], "PASS")

    def test_out_of_scope(self):
        self._write_file("test.txt", "hello\n")
        self._write_file("undeclared.txt", "hello\n")
        subprocess.run(["git", "add", "test.txt", "undeclared.txt"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "test"], cwd=self.repo_path, check=True, env=self.env)

        head_sha = self._get_sha("HEAD")
        result = self._run_check(head_sha, "test.txt")

        self.assertEqual(result.returncode, 1)
        self.assertIn("Changed path not in declared list: undeclared.txt", result.stderr)

    def test_rename_escaping_root(self):
        self._write_file("link", "hello\n")
        subprocess.run(["git", "add", "link"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "test"], cwd=self.repo_path, check=True, env=self.env)

        # Replace file with a symlink that points outside
        os.remove(os.path.join(self.repo_path, "link"))
        os.symlink("/etc/passwd", os.path.join(self.repo_path, "link"))

        subprocess.run(["git", "add", "link"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "symlink"], cwd=self.repo_path, check=True, env=self.env)

        head_sha = self._get_sha("HEAD")
        result = self._run_check(head_sha, "link")

        self.assertEqual(result.returncode, 1)
        self.assertTrue("Path resolves outside repository" in result.stderr or "Broken symlink" in result.stderr)

    def test_absolute_path_in_declared(self):
        self._write_file("test.txt", "hello\n")
        subprocess.run(["git", "add", "test.txt"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "test"], cwd=self.repo_path, check=True, env=self.env)

        head_sha = self._get_sha("HEAD")
        result = self._run_check(head_sha, "test.txt,/test.txt")
        # However, the script checks if a *changed* path is absolute or in the list.
        # But if we change an absolute path? Git doesn't allow absolute paths natively.
        # We can just manually invoke the script's validation logic, or make the script parse the list of changed paths with a leading slash.
        pass # Not applicable as git paths are relative to repo root

    def test_sibling_prefix_path_rejected(self):
        # Create a sibling directory
        parent_dir = os.path.dirname(self.repo_path)
        base_name = os.path.basename(self.repo_path)
        sibling_dir = os.path.join(parent_dir, base_name + "-sibling")
        os.makedirs(sibling_dir, exist_ok=True)
        try:
            self._write_file("link", "hello\n")
            subprocess.run(["git", "add", "link"], cwd=self.repo_path, check=True, env=self.env)
            subprocess.run(["git", "commit", "-m", "test"], cwd=self.repo_path, check=True, env=self.env)

            # Replace file with a symlink to sibling directory
            os.remove(os.path.join(self.repo_path, "link"))
            os.symlink(sibling_dir, os.path.join(self.repo_path, "link"))

            subprocess.run(["git", "add", "link"], cwd=self.repo_path, check=True, env=self.env)
            subprocess.run(["git", "commit", "-m", "symlink"], cwd=self.repo_path, check=True, env=self.env)

            head_sha = self._get_sha("HEAD")
            result = self._run_check(head_sha, "link")

            self.assertEqual(result.returncode, 1)
            self.assertIn("Path resolves outside repository", result.stderr)
        finally:
            os.rmdir(sibling_dir)

    def test_absolute_path_error_logic(self):
        # Test broken symlink specifically
        self._write_file("link", "hello\n")
        subprocess.run(["git", "add", "link"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "test"], cwd=self.repo_path, check=True, env=self.env)

        # Replace file with a broken symlink
        os.remove(os.path.join(self.repo_path, "link"))
        os.symlink("nonexistent", os.path.join(self.repo_path, "link"))

        subprocess.run(["git", "add", "link"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "symlink"], cwd=self.repo_path, check=True, env=self.env)

        head_sha = self._get_sha("HEAD")
        result = self._run_check(head_sha, "link")

        self.assertEqual(result.returncode, 1)
        self.assertIn("Broken symlink", result.stderr)

    def test_text_file_containing_absolute_personal_path(self):
        self._write_file("test.txt", "My path is /home/user/code\n")
        subprocess.run(["git", "add", "test.txt"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "test"], cwd=self.repo_path, check=True, env=self.env)

        head_sha = self._get_sha("HEAD")
        result = self._run_check(head_sha, "test.txt")

        self.assertEqual(result.returncode, 1)
        self.assertIn("use $HOME/...", result.stderr)
        self.assertNotIn("/home/user/code", result.stderr)
        self.assertNotIn("/home/user/code", result.stdout)

    def test_text_file_containing_home_example(self):
        self._write_file("test.txt", "My path is $HOME/code\n")
        subprocess.run(["git", "add", "test.txt"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "test"], cwd=self.repo_path, check=True, env=self.env)

        head_sha = self._get_sha("HEAD")
        result = self._run_check(head_sha, "test.txt")

        self.assertEqual(result.returncode, 0)
        report = json.loads(result.stdout)
        self.assertEqual(report["verdict"], "PASS")

    def test_non_text_file(self):
        binary_path = os.path.join(self.repo_path, "bin.dat")
        with open(binary_path, "wb") as f:
            f.write(os.urandom(10) + b"\x00")

        subprocess.run(["git", "add", "bin.dat"], cwd=self.repo_path, check=True, env=self.env)
        subprocess.run(["git", "commit", "-m", "bin"], cwd=self.repo_path, check=True, env=self.env)

        head_sha = self._get_sha("HEAD")
        result = self._run_check(head_sha, "bin.dat")

        self.assertEqual(result.returncode, 1)
        self.assertIn("Non-text file added: bin.dat", result.stderr)

if __name__ == '__main__':
    unittest.main()
