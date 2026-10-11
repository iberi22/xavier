import importlib.util
from io import StringIO
import os
import sys
import tempfile
import unittest
from unittest.mock import patch

class TestAcceptancePackage(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = os.path.join(os.path.dirname(os.path.dirname(__file__)), "acceptance-package.py")
        spec = importlib.util.spec_from_file_location("acceptance_package", path)
        cls.mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.mod)

    def run_main(self, gh_res=None, git_res=None, sha="abcdef1234567890", out=None, apply=False):
        gh_map = gh_res or {}
        git_map = git_res or {}
        def mock_gh(cmd):
            s = " ".join(cmd)
            for k, v in gh_map.items():
                if k in s:
                    if isinstance(v, Exception):
                        raise v
                    return v
            return []
        def mock_git(cmd):
            s = " ".join(cmd)
            for k, v in git_map.items():
                if k in s:
                    return v
            return ""

        args = type("Args", (), {"sha": sha, "out": out, "apply": apply})()
        with patch("sys.stdout", new_callable=StringIO) as out_buf:
            self.mod.main(args, mock_gh, mock_git)
            return out_buf.getvalue()

    def test_acceptance_cases(self):
        pr_std = [{"body": "## Changes\nSome change\n## Trial steps\nStep 1\n## Results\nSuccess\n## Risks\nNone\n## Links\nNone", "title": "Test PR"}]
        run_ok = [{"status": "completed", "conclusion": "success", "name": "CI"}]

        # 1. ready_for_acceptance
        with self.subTest("ready_for_acceptance"):
            out = self.run_main(
                gh_res={"gh pr list": pr_std, "gh run list": run_ok, "gh issue list": [], "gh api": ""},
                git_res={"rev-parse HEAD": "abcdef1234567890", "describe --tags": "v1.0.0"}
            )
            self.assertIn("READY_FOR_ACCEPTANCE", out)
            self.assertIn("All checks passed.", out)
            self.assertIn("v1.0.0", out)
            self.assertIn("git revert abcdef1234567890", out)

        # 2. not_ready_ci_failed
        with self.subTest("not_ready_ci_failed"):
            out = self.run_main(
                gh_res={"gh pr list": [{"body": "## Changes\nSome change\n## Trial steps\nStep 1\n## Results\nSuccess", "title": "Test PR"}],
                        "gh run list": [{"status": "completed", "conclusion": "failure", "name": "CI"}],
                        "gh issue list": [], "gh api": ""}
            )
            self.assertIn("NOT_READY", out)
            self.assertIn("CI run 'CI' failed or was cancelled.", out)

        # 3. not_ready_open_incident
        with self.subTest("not_ready_open_incident"):
            out = self.run_main(
                gh_res={"gh pr list": [{"body": "## Changes\nSome change\n## Trial steps\nStep 1\n## Results\nSuccess", "title": "Test PR"}],
                        "gh run list": run_ok,
                        "gh issue list": [{"body": "<!-- tbd-incident sha=abcdef1 workflow=CI -->"}],
                        "gh api": ""}
            )
            self.assertIn("NOT_READY", out)
            self.assertIn("Open tbd-incident issues exist for this SHA", out)

        # 4. broken_link
        with self.subTest("broken_link"):
            out = self.run_main(
                gh_res={"gh pr list": [{"body": "## Changes\nC\n## Trial steps\nT\n## Results\nR\n## Links\n[bad](https://github.com/iberi22/xavier/issues/999999999)", "title": "Test PR"}],
                        "gh run list": run_ok, "gh issue list": [],
                        "gh api https://github.com/iberi22/xavier/issues/999999999": Exception("Failed")}
            )
            self.assertIn("NOT_READY", out)
            self.assertIn("Broken links found: https://github.com/iberi22/xavier/issues/999999999", out)

        # 5. new_headings_pr_body
        with self.subTest("new_headings_pr_body"):
            out = self.run_main(
                gh_res={"gh pr list": [{"body": "## What changed\nNew feature changes\n## How it was tested\nRan unit tests\n## Risks\nNone",
                                        "title": "Test PR", "url": "https://github.com/iberi22/xavier/pull/123"}],
                        "gh run list": [{"status": "completed", "conclusion": "success", "name": "CI", "url": "https://github.com/iberi22/xavier/actions/runs/456"}],
                        "gh issue list": [], "gh api": ""}
            )
            self.assertIn("READY_FOR_ACCEPTANCE", out)
            self.assertIn("## Changes\nNew feature changes", out)
            self.assertIn("## Trial steps\nRan unit tests", out)
            self.assertIn("## Results\nRan unit tests", out)

        # 6. rollback_merge_commit_and_single_parent
        with self.subTest("rollback_merge_commit_and_single_parent"):
            base_gh = {"gh pr list": [{"body": "## Changes\nC\n## Trial steps\nT\n## Results\nR", "title": "Test PR"}],
                       "gh run list": run_ok, "gh issue list": [], "gh api": ""}
            out_merge = self.run_main(gh_res=base_gh, git_res={"rev-list --parents": "commit_sha parent_sha1 parent_sha2"}, sha="commit_sha")
            self.assertIn("git revert -m 1 commit_sha", out_merge)
            out_single = self.run_main(gh_res=base_gh, git_res={"rev-list --parents": "commit_sha parent_sha1"}, sha="commit_sha")
            self.assertIn("git revert commit_sha", out_single)
            self.assertNotIn("git revert -m 1 commit_sha", out_single)

        # 7. links_listed
        with self.subTest("links_listed"):
            out = self.run_main(
                gh_res={"gh pr list": [{"body": "## Changes\nC\n## Trial steps\nT\n## Results\nR", "title": "Test PR", "url": "https://github.com/iberi22/xavier/pull/2933"}],
                        "gh run list": [{"status": "completed", "conclusion": "success", "name": "Build", "url": "https://github.com/iberi22/xavier/actions/runs/1001"},
                                        {"status": "completed", "conclusion": "success", "name": "Test", "url": "https://github.com/iberi22/xavier/actions/runs/1002"}],
                        "gh issue list": [], "gh api": ""}
            )
            self.assertIn("## Links\n- PR: https://github.com/iberi22/xavier/pull/2933\n- CI (Build): https://github.com/iberi22/xavier/actions/runs/1001\n- CI (Test): https://github.com/iberi22/xavier/actions/runs/1002", out)

    def test_file_writing(self):
        gh = {"gh pr list": [{"body": "## Changes\nC\n## Trial steps\nT\n## Results\nR", "title": "Test PR"}],
              "gh run list": [{"status": "completed", "conclusion": "success", "name": "CI"}],
              "gh issue list": [], "gh api": ""}
        with tempfile.TemporaryDirectory() as temp_dir:
            out_file = os.path.join(temp_dir, "output.md")
            out = self.run_main(gh_res=gh, out=out_file, apply=False)
            self.assertIn("DRY-RUN:", out)
            self.assertFalse(os.path.exists(out_file) and os.path.getsize(out_file) > 0)

            self.run_main(gh_res=gh, out=out_file, apply=True)
            self.assertTrue(os.path.exists(out_file))
            with open(out_file, "r", encoding="utf-8") as f:
                content = f.read()
                self.assertIn("READY_FOR_ACCEPTANCE", content)
                self.assertIn("## Changes\nC", content)

if __name__ == "__main__":
    unittest.main()
