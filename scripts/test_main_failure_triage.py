import importlib.util
import os
import sys
import unittest
from typing import List, Tuple

MODULE_PATH = os.path.join(os.path.dirname(__file__), "main-failure-triage.py")
spec = importlib.util.spec_from_file_location("main_failure_triage", MODULE_PATH)
mod = importlib.util.module_from_spec(spec)
sys.modules["main_failure_triage"] = mod
spec.loader.exec_module(mod)
triage_failure = mod.triage_failure
matches_forbidden_pattern = mod.matches_forbidden_pattern


class TestMainFailureTriage(unittest.TestCase):
    def test_forbidden_patterns(self):
        for p in ["src/storage/migrations.rs", "db/migrations/001.sql", "migrations/001.sql", ".github/workflows/release.yml"]:
            self.assertTrue(matches_forbidden_pattern(p))
        for p in ["src/main.rs", ".github/workflows/jules-ci-feedback.yml"]:
            self.assertFalse(matches_forbidden_pattern(p))

    def test_single_safe_commit_dry_run(self):
        cmds: List[List[str]] = []
        def runner(c: List[str]) -> Tuple[int, str, str]:
            cmds.append(c)
            return (0, "c1\n", "") if "rev-list" in c else (0, "src/lib.rs\n", "") if "diff" in c else (0, "[]", "") if c[:3] == ["gh", "pr", "list"] else (0, "", "")

        safe, msg = triage_failure("c10000000000", "CI", 1, True, "g00000000000", runner)
        self.assertTrue(safe)
        self.assertIn("SAFE: Planned revert PR", msg)
        self.assertTrue(all("checkout" not in c and "push" not in c for c in cmds))

    def test_single_safe_commit_live(self):
        cmds: List[List[str]] = []
        def runner(c: List[str]) -> Tuple[int, str, str]:
            cmds.append(c)
            if "rev-list" in c: return 0, "c1\n", ""
            if "diff" in c: return 0, "src/lib.rs\n", ""
            if "log" in c: return 0, "fix typo\n", ""
            if c[:3] == ["gh", "pr", "list"]: return 0, "[]", ""
            if c[:3] == ["gh", "pr", "create"]: return 0, "https://github.com/pr/1\n", ""
            return 0, "", ""

        safe, msg = triage_failure("c10000000000", "CI", 1, False, "g00000000000", runner)
        self.assertTrue(safe)
        self.assertIn("Created revert PR", msg)
        self.assertIn(["git", "checkout", "-b", "revert/c100000", "main"], cmds)
        self.assertIn(["git", "revert", "--no-edit", "c10000000000"], cmds)
        self.assertIn(["git", "push", "origin", "revert/c100000"], cmds)

    def test_single_safe_merge_commit_reverts_with_m1(self):
        cmds: List[List[str]] = []
        def runner(c: List[str]) -> Tuple[int, str, str]:
            cmds.append(c)
            if "rev-list" in c: return 0, "m1\nc2\nc1\n", ""
            if "rev-parse" in c: return 0, "p1\np2\n", ""
            if "diff" in c: return 0, "src/server.rs\n", ""
            if "log" in c: return 0, "Merge PR #5\n", ""
            if c[:3] == ["gh", "pr", "list"]: return 0, "[]", ""
            return 0, "", ""

        safe, _ = triage_failure("m10000000000", "CI", 1, False, "g00000000000", runner)
        self.assertTrue(safe)
        self.assertIn(["git", "revert", "--no-edit", "-m", "1", "m10000000000"], cmds)

    def test_two_commits_since_last_green_no_revert(self):
        def runner(c: List[str]) -> Tuple[int, str, str]:
            return (0, "c2\nc1\n", "") if "rev-list" in c else (0, "c1\n", "") if "rev-parse" in c else (0, "", "")

        safe, msg = triage_failure("c20000000000", "CI", 1, True, "g00000000000", runner)
        self.assertFalse(safe)
        self.assertIn("Multiple commits (2) found", msg)

    def test_migration_touched_no_revert(self):
        def runner(c: List[str]) -> Tuple[int, str, str]:
            return (0, "c1\n", "") if "rev-list" in c else (0, "src/storage/migrations.rs\n", "") if "diff" in c else (0, "", "")

        safe, msg = triage_failure("c10000000000", "CI", 1, True, "g00000000000", runner)
        self.assertFalse(safe)
        self.assertIn("protected/migration files", msg)

    def test_release_workflow_touched_no_revert(self):
        def runner(c: List[str]) -> Tuple[int, str, str]:
            return (0, "c1\n", "") if "rev-list" in c else (0, ".github/workflows/release.yml\n", "") if "diff" in c else (0, "", "")

        safe, msg = triage_failure("c10000000000", "CI", 1, True, "g00000000000", runner)
        self.assertFalse(safe)
        self.assertIn("protected/migration files", msg)

    def test_tagged_commit_no_revert(self):
        def runner(c: List[str]) -> Tuple[int, str, str]:
            return (0, "c1\n", "") if "rev-list" in c else (0, "src/main.rs\n", "") if "diff" in c else (0, "v1.0.0\n", "") if "tag" in c else (0, "", "")

        safe, msg = triage_failure("c10000000000", "CI", 1, True, "g00000000000", runner)
        self.assertFalse(safe)
        self.assertIn("Commit is tagged", msg)

    def test_cargo_lock_release_bump_no_revert(self):
        def runner(c: List[str]) -> Tuple[int, str, str]:
            if "rev-list" in c: return 0, "c1\n", ""
            if "diff" in c and "--name-only" in c: return 0, "Cargo.lock\n", ""
            if "diff" in c and "--" in c: return 0, "cargo-release = 1\n", ""
            return 0, "", ""

        safe, msg = triage_failure("c10000000000", "CI", 1, True, "g00000000000", runner)
        self.assertFalse(safe)
        self.assertIn("Cargo.lock release tooling", msg)

    def test_already_open_revert_pr_idempotent(self):
        def runner(c: List[str]) -> Tuple[int, str, str]:
            if "rev-list" in c: return 0, "c1\n", ""
            if "diff" in c: return 0, "src/main.rs\n", ""
            if c[:3] == ["gh", "pr", "list"]: return 0, '[{"number": 99}]', ""
            return 0, "", ""

        safe, msg = triage_failure("c10000000000", "CI", 1, False, "g00000000000", runner)
        self.assertTrue(safe)
        self.assertIn("already open (idempotent)", msg)


if __name__ == "__main__":
    unittest.main()
