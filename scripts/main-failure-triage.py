#!/usr/bin/env python3
"""Triage main CI failures and determine if an automated revert PR can be opened safely."""
import argparse
import fnmatch
import json
import os
import re
import subprocess
import sys
from typing import Callable, List, Optional, Tuple

FORBIDDEN_PATH_PATTERNS = [
    "src/storage/migrations.rs",
    "*/migrations/*",
    "*migrations/*",
    "migrations/*",
    ".github/workflows/release*",
]


def default_runner(cmd: List[str]) -> Tuple[int, str, str]:
    r = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    return r.returncode, r.stdout, r.stderr


def matches_forbidden_pattern(path: str) -> bool:
    norm = path.strip().replace("\\", "/")
    return any(fnmatch.fnmatch(norm, p) for p in FORBIDDEN_PATH_PATTERNS)


def get_last_green_sha(
    workflow: str, sha: str, runner: Callable[[List[str]], Tuple[int, str, str]]
) -> Optional[str]:
    c, out, _ = runner([
        "gh", "run", "list", "--workflow", workflow, "--branch", "main",
        "--status", "success", "--event", "push", "--limit", "20", "--json", "headSha"
    ])
    if c != 0 or not out.strip():
        return None
    try:
        for r in json.loads(out):
            s = r.get("headSha")
            if s and s != sha:
                return s
    except Exception:
        pass
    return None


def get_incident_issue(
    sha: str, workflow: str, runner: Callable[[List[str]], Tuple[int, str, str]]
) -> Optional[int]:
    c, out, _ = runner([
        "gh", "issue", "list", "--label", "tbd-incident", "--state", "open",
        "--limit", "200", "--json", "number,body"
    ])
    if c != 0 or not out.strip():
        return None
    try:
        pat = re.compile(r"<!--\s*tbd-incident\s+sha=([a-zA-Z0-9]+)\s+workflow=(.+?)\s*-->")
        for iss in json.loads(out):
            m = pat.search(iss.get("body", ""))
            if m and m.group(1) == sha and (not workflow or m.group(2).strip() == workflow.strip()):
                return int(iss.get("number"))
    except Exception:
        pass
    return None


def is_revert_pr_open(branch: str, runner: Callable[[List[str]], Tuple[int, str, str]]) -> bool:
    c, out, _ = runner(["gh", "pr", "list", "--head", branch, "--state", "open", "--json", "number"])
    if c != 0 or not out.strip():
        return False
    try:
        return len(json.loads(out)) > 0
    except Exception:
        return False


def triage_failure(
    failing_sha: str,
    workflow_name: str,
    incident_issue: Optional[int],
    dry_run: bool,
    last_green: Optional[str] = None,
    runner: Callable[[List[str]], Tuple[int, str, str]] = default_runner,
) -> Tuple[bool, str]:
    if not last_green:
        last_green = get_last_green_sha(workflow_name, failing_sha, runner)
    if not last_green:
        return False, "No previous green run found on main for workflow to compare against."

    c, out, _ = runner(["git", "rev-list", f"{last_green}..{failing_sha}"])
    if c != 0:
        return False, f"Failed to inspect git rev-list for {last_green}..{failing_sha}."
    commits = [x.strip() for x in out.strip().splitlines() if x.strip()]
    if not commits:
        return False, "Failing SHA is already equal to or behind last green run."

    c, parents_out, _ = runner(["git", "rev-parse", f"{failing_sha}^@"])
    is_merge = c == 0 and len([p for p in parents_out.strip().splitlines() if p.strip()]) > 1

    if len(commits) > 1 and not is_merge:
        return False, f"Multiple commits ({len(commits)}) found since last green run ({last_green[:7]}). Automatic single-commit revert is unsafe."

    diff_range = f"{failing_sha}^1..{failing_sha}" if is_merge else f"{failing_sha}~1..{failing_sha}"
    c, diff_files, _ = runner(["git", "diff", "--name-only", diff_range])
    if c != 0:
        c, diff_files, _ = runner(["git", "show", "--pretty=", "--name-only", failing_sha])
    files = [f.strip() for f in diff_files.strip().splitlines() if f.strip()]

    forbidden = [f for f in files if matches_forbidden_pattern(f)]
    if forbidden:
        return False, f"Failing commit touched protected/migration files: {', '.join(forbidden)}."

    if files == ["Cargo.lock"]:
        c, lock_diff, _ = runner(["git", "diff", diff_range, "--", "Cargo.lock"])
        if c == 0 and ("cargo-release" in lock_diff or "release" in lock_diff):
            return False, "Commit only bumps Cargo.lock release tooling; automatic revert unsafe."

    c, tags_out, _ = runner(["git", "tag", "--points-at", failing_sha])
    if c == 0 and tags_out.strip():
        return False, f"Commit is tagged ({tags_out.strip()}); automatic revert is unsafe."

    short_sha = failing_sha[:7]
    branch = f"revert/{short_sha}"
    if is_revert_pr_open(branch, runner):
        return True, f"Revert PR for branch {branch} is already open (idempotent)."

    if dry_run:
        return True, f"SAFE: Planned revert PR on branch {branch} for commit {short_sha}."

    c, _, err = runner(["git", "checkout", "-b", branch, "main"])
    if c != 0:
        return False, f"Failed to checkout branch {branch}: {err}"

    revert_cmd = ["git", "revert", "--no-edit"] + (["-m", "1"] if is_merge else []) + [failing_sha]
    c, _, err = runner(revert_cmd)
    if c != 0:
        return False, f"git revert failed: {err}"

    c, _, err = runner(["git", "push", "origin", branch])
    if c != 0:
        return False, f"Failed to push branch {branch}: {err}"

    c, orig_title, _ = runner(["git", "log", "-1", "--format=%s", failing_sha])
    title = f"revert: {orig_title.strip() if c == 0 and orig_title.strip() else short_sha}"
    body = f"Automated revert of {failing_sha} due to main CI failure.\n\n"
    if incident_issue:
        body += f"Fixes/Relates to incident #{incident_issue}.\n"

    c, out, err = runner(["gh", "pr", "create", "--title", title, "--body", body, "--head", branch, "--base", "main"])
    if c != 0:
        return False, f"Failed to open revert PR: {err}"
    return True, f"Created revert PR: {out.strip()}"


def main() -> None:
    p = argparse.ArgumentParser(description="Triage main CI failures and safely propose revert PRs.")
    p.add_argument("--sha", required=True, help="Failing commit SHA on main.")
    p.add_argument("--workflow", default=os.environ.get("WORKFLOW_NAME", "Xavier CI / CD"), help="Workflow name.")
    p.add_argument("--incident-issue", type=int, default=None, help="Incident issue number if already known.")
    p.add_argument("--last-green", default=None, help="Last green commit SHA (optional).")
    p.add_argument("--dry-run", action="store_true", default=False, help="Print decision only without modifying branches or opening PRs.")
    args = p.parse_args()

    incident_issue = args.incident_issue or get_incident_issue(args.sha, args.workflow, default_runner)
    safe, exp = triage_failure(
        failing_sha=args.sha, workflow_name=args.workflow, incident_issue=incident_issue,
        dry_run=args.dry_run, last_green=args.last_green, runner=default_runner
    )
    print(exp)
    if not safe and incident_issue and not args.dry_run:
        comment = f"⚠️ **Automated Revert Skipped**\n\nCommit {args.sha[:7]} could not be automatically reverted:\n> {exp}"
        default_runner(["gh", "issue", "comment", str(incident_issue), "--body", comment])
    sys.exit(0 if safe else 1)


if __name__ == "__main__":
    main()
