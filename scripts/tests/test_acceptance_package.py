import unittest
import os
import sys
import tempfile
import importlib.util
from io import StringIO

class TestAcceptancePackage(unittest.TestCase):
    def setUp(self):
        script_dir = os.path.dirname(os.path.dirname(__file__))
        script_path = os.path.join(script_dir, "acceptance-package.py")
        spec = importlib.util.spec_from_file_location("acceptance_package", script_path)
        self.module = importlib.util.module_from_spec(spec)
        sys.modules["acceptance_package"] = self.module
        spec.loader.exec_module(self.module)

    def tearDown(self):
        if "acceptance_package" in sys.modules:
            del sys.modules["acceptance_package"]

    def create_mock_runners(self, gh_responses=None, git_responses=None):
        if gh_responses is None:
            gh_responses = {}
        if git_responses is None:
            git_responses = {}

        def mock_gh_runner(cmd):
            cmd_str = " ".join(cmd)
            for k, v in gh_responses.items():
                if k in cmd_str:
                    if isinstance(v, Exception):
                        raise v
                    return v
            return []

        def mock_git_runner(cmd):
            cmd_str = " ".join(cmd)
            for k, v in git_responses.items():
                if k in cmd_str:
                    return v
            return ""

        return mock_gh_runner, mock_git_runner

    def test_ready_for_acceptance(self):
        gh_responses = {
            "gh pr list": [{"body": "## Changes\nSome change\n## Trial steps\nStep 1\n## Results\nSuccess\n## Risks\nNone\n## Links\nNone", "title": "Test PR"}],
            "gh run list": [{"status": "completed", "conclusion": "success", "name": "CI"}],
            "gh issue list": [],
            "gh api": ""
        }
        git_responses = {
            "rev-parse HEAD": "abcdef1234567890",
            "describe --tags": "v1.0.0"
        }

        mock_gh_runner, mock_git_runner = self.create_mock_runners(gh_responses, git_responses)

        class MockArgs:
            sha = "abcdef1234567890"
            out = None

        # Capture stdout
        from io import StringIO
        import sys
        from unittest.mock import patch

        with patch('sys.stdout', new_callable=StringIO) as mock_stdout:
            self.module.main(MockArgs(), mock_gh_runner, mock_git_runner)
            output = mock_stdout.getvalue()

        self.assertIn("READY_FOR_ACCEPTANCE", output)
        self.assertIn("All checks passed.", output)
        self.assertIn("v1.0.0", output)
        self.assertIn("git revert abcdef1234567890", output)

    def test_not_ready_ci_failed(self):
        gh_responses = {
            "gh pr list": [{"body": "## Changes\nSome change\n## Trial steps\nStep 1\n## Results\nSuccess", "title": "Test PR"}],
            "gh run list": [{"status": "completed", "conclusion": "failure", "name": "CI"}],
            "gh issue list": [],
            "gh api": ""
        }
        mock_gh_runner, mock_git_runner = self.create_mock_runners(gh_responses)

        class MockArgs:
            sha = "abcdef1234567890"
            out = None

        from unittest.mock import patch
        with patch('sys.stdout', new_callable=StringIO) as mock_stdout:
            self.module.main(MockArgs(), mock_gh_runner, mock_git_runner)
            output = mock_stdout.getvalue()

        self.assertIn("NOT_READY", output)
        self.assertIn("CI run 'CI' failed or was cancelled.", output)

    def test_not_ready_open_incident(self):
        gh_responses = {
            "gh pr list": [{"body": "## Changes\nSome change\n## Trial steps\nStep 1\n## Results\nSuccess", "title": "Test PR"}],
            "gh run list": [{"status": "completed", "conclusion": "success", "name": "CI"}],
            "gh issue list": [{"body": "<!-- tbd-incident sha=abcdef1 workflow=CI -->"}],
            "gh api": ""
        }
        mock_gh_runner, mock_git_runner = self.create_mock_runners(gh_responses)

        class MockArgs:
            sha = "abcdef1234567890"
            out = None

        from unittest.mock import patch
        with patch('sys.stdout', new_callable=StringIO) as mock_stdout:
            self.module.main(MockArgs(), mock_gh_runner, mock_git_runner)
            output = mock_stdout.getvalue()

        self.assertIn("NOT_READY", output)
        self.assertIn("Open tbd-incident issues exist for this SHA", output)

    def test_file_writing(self):
        gh_responses = {
            "gh pr list": [{"body": "## Changes\nC\n## Trial steps\nT\n## Results\nR", "title": "Test PR"}],
            "gh run list": [{"status": "completed", "conclusion": "success", "name": "CI"}],
            "gh issue list": [],
            "gh api": ""
        }
        mock_gh_runner, mock_git_runner = self.create_mock_runners(gh_responses)

        with tempfile.TemporaryDirectory() as temp_dir:
            out_file = os.path.join(temp_dir, "output.md")

            class MockArgsDryRun:
                sha = "abcdef1234567890"
                out = out_file
                apply = False

            from unittest.mock import patch
            with patch('sys.stdout', new_callable=StringIO) as mock_stdout:
                self.module.main(MockArgsDryRun(), mock_gh_runner, mock_git_runner)
                output = mock_stdout.getvalue()
                self.assertIn("DRY-RUN:", output)
            self.assertFalse(os.path.exists(out_file) and os.path.getsize(out_file) > 0)

            class MockArgsApply:
                sha = "abcdef1234567890"
                out = out_file
                apply = True

            self.module.main(MockArgsApply(), mock_gh_runner, mock_git_runner)

            self.assertTrue(os.path.exists(out_file))
            with open(out_file, "r", encoding="utf-8") as f:
                content = f.read()
                self.assertIn("READY_FOR_ACCEPTANCE", content)
                self.assertIn("## Changes\nC", content)

    def test_broken_link(self):
        gh_responses = {
            "gh pr list": [{"body": "## Changes\nC\n## Trial steps\nT\n## Results\nR\n## Links\n[bad](https://github.com/iberi22/xavier/issues/999999999)", "title": "Test PR"}],
            "gh run list": [{"status": "completed", "conclusion": "success", "name": "CI"}],
            "gh issue list": [],
            "gh api https://github.com/iberi22/xavier/issues/999999999": Exception("Failed")
        }
        mock_gh_runner, mock_git_runner = self.create_mock_runners(gh_responses)

        class MockArgs:
            sha = "abcdef1234567890"
            out = None

        from unittest.mock import patch
        with patch('sys.stdout', new_callable=StringIO) as mock_stdout:
            self.module.main(MockArgs(), mock_gh_runner, mock_git_runner)
            output = mock_stdout.getvalue()

        self.assertIn("NOT_READY", output)
        self.assertIn("Broken links found: https://github.com/iberi22/xavier/issues/999999999", output)

if __name__ == '__main__':
    from io import StringIO
    unittest.main()
