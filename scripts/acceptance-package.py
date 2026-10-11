#!/usr/bin/env python3
import argparse
import json
import subprocess
import re
import sys
import os
import urllib.request
from datetime import datetime, timezone
import tempfile

def default_gh_runner(cmd):
    """Executes a gh command and parses JSON output if applicable."""
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        if result.stdout.strip() == "":
            return None
        try:
            return json.loads(result.stdout)
        except json.JSONDecodeError:
            return result.stdout.strip()
    except subprocess.CalledProcessError as e:
        stderr_first_line = e.stderr.strip().split('\n')[0] if e.stderr else "Unknown error"
        print(f"ERROR: {' '.join(cmd)} failed: {stderr_first_line}", file=sys.stderr)
        sys.exit(2)

def default_git_runner(cmd):
    """Executes a git command and returns text output."""
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        return result.stdout.strip()
    except subprocess.CalledProcessError as e:
        return ""

def get_args():
    parser = argparse.ArgumentParser(description="Generate Product Acceptance Package")
    parser.add_argument("--sha", help="Target SHA", default=None)
    parser.add_argument("--out", help="Output file", default=None)
    parser.add_argument("--apply", help="Apply file writes (dry-run is default)", action="store_true")
    return parser.parse_args()

def check_path_allowlist(path):
    try:
        resolved = os.path.realpath(path)
        workspace = os.path.realpath(os.getcwd())
        # Check against workspace or temp directory for tests
        if not resolved.startswith(workspace) and not resolved.startswith(os.path.realpath(tempfile.gettempdir())):
            print(f"ERROR: Path {path} resolves outside allowlisted workspace root.", file=sys.stderr)
            sys.exit(2)
        return resolved
    except Exception as e:
        print(f"ERROR: Path resolution failed: {e}", file=sys.stderr)
        sys.exit(2)

def extract_section(body, section_name):
    if not body:
        return ""
    # Look for a section starting with exactly '## Section'
    # We want to match until the next '## ' header or end of string.
    pattern = rf"(?:^|\n)## {section_name}\s*(.*?)(?=\n## |\Z)"
    match = re.search(pattern, body, re.DOTALL | re.IGNORECASE)
    if match:
        return match.group(1).strip()
    return ""

def main(args, gh_runner=default_gh_runner, git_runner=default_git_runner):
    if not args.sha:
        args.sha = git_runner(["git", "rev-parse", "HEAD"])

    if not args.sha:
        print("ERROR: Could not determine SHA", file=sys.stderr)
        sys.exit(2)

    sha = args.sha

    # Fetch PR data
    pr_data = gh_runner(["gh", "pr", "list", "--state", "merged", "--search", sha, "--json", "number,title,url,body"])
    pr = pr_data[0] if pr_data else None

    body = pr["body"] if pr else ""
    changes = extract_section(body, "Changes")
    trial_steps = extract_section(body, "Trial steps")
    results = extract_section(body, "Results")
    risks = extract_section(body, "Risks")
    links_text = extract_section(body, "Links")

    # Fetch CI runs
    runs_data = gh_runner(["gh", "run", "list", "--commit", sha, "--json", "conclusion,name,url,status"])
    runs = runs_data if runs_data else []

    # Fetch tbd-incidents
    incidents_data = gh_runner(["gh", "issue", "list", "--label", "tbd-incident", "--state", "open", "--json", "number,body"])
    incidents = incidents_data if incidents_data else []

    open_incidents_for_sha = []
    for inc in incidents:
        inc_body = inc.get("body", "")
        # <!-- tbd-incident sha=<SHA> workflow=<NAME> -->
        match = re.search(r"<!-- tbd-incident sha=([a-zA-Z0-9]+) workflow=(.+) -->", inc_body)
        if match:
            inc_sha = match.group(1)
            if inc_sha.startswith(sha) or sha.startswith(inc_sha):
                open_incidents_for_sha.append(inc)

    # Determine rollbacks
    source_rollback = f"git revert {sha}" if len(sha) >= 7 else "git revert <merge_commit_sha>"
    prev_tag = git_runner(["git", "describe", "--tags", "--abbrev=0", f"{sha}^"])
    deployed_rollback = prev_tag if prev_tag else "unknown"

    # Link validation
    broken_links = []
    # Find markdown links [text](url) or bare urls
    link_urls = re.findall(r'\[.*?\]\((https?://.*?)\)|(https?://[^\s]+)', links_text)
    link_urls = [u[0] if u[0] else u[1] for u in link_urls if u[0] or u[1]]

    for url in link_urls:
        try:
            gh_runner(["gh", "api", url, "-X", "GET", "--silent"])
        except SystemExit:
            broken_links.append(url)
        except Exception:
            broken_links.append(url)

    # Readiness Decision
    not_ready_reasons = []

    if not runs:
        not_ready_reasons.append("No CI runs found for this SHA.")
    else:
        for r in runs:
            # if status is completed and conclusion is success or skipped
            if r.get("status") != "completed":
                not_ready_reasons.append(f"CI run '{r.get('name')}' is not completed.")
            elif r.get("conclusion") not in ("success", "skipped"):
                not_ready_reasons.append(f"CI run '{r.get('name')}' failed or was cancelled.")

    if open_incidents_for_sha:
        not_ready_reasons.append(f"Open tbd-incident issues exist for this SHA.")

    if broken_links:
        not_ready_reasons.append(f"Broken links found: {', '.join(broken_links)}")

    if not changes or not trial_steps or not results:
        not_ready_reasons.append("Missing required evidence sections (Changes, Trial steps, Results) in PR body.")

    if not_ready_reasons:
        decision = "NOT_READY"
        decision_details = "\n".join(f"- {reason}" for reason in not_ready_reasons)
    else:
        decision = "READY_FOR_ACCEPTANCE"
        decision_details = "All checks passed."

    # Build Markdown
    markdown = f"# Product Acceptance Package for {sha}\n\n"
    markdown += f"## Changes\n{changes or 'N/A'}\n\n"
    markdown += f"## Trial steps\n{trial_steps or 'N/A'}\n\n"
    markdown += f"## Results\n{results or 'N/A'}\n\n"
    markdown += f"## Risks\n{risks or 'N/A'}\n\n"
    markdown += f"## Rollback\n"
    markdown += f"### Source rollback\n`{source_rollback}`\n\n"
    markdown += f"### Deployed binary / tag rollback\n`{deployed_rollback}`\n\n"
    markdown += f"## Links\n{links_text or 'N/A'}\n\n"
    markdown += f"## Decision\n**{decision}**\n\n{decision_details}\n"

    # Write output
    if args.out:
        out_path = check_path_allowlist(args.out)

        if not args.apply:
            print(f"DRY-RUN: Would write output to {out_path}")
            print(f"DRY-RUN content snippet:\n{markdown[:200]}...")
            return

        import time
        if os.path.exists(out_path) and os.path.getsize(out_path) > 0:
            backup_path = f"{out_path}.bak-{int(time.time())}"
            try:
                os.rename(out_path, backup_path)
            except Exception:
                pass # Proceed anyway if backup fails

        tmp_path = f"{out_path}.tmp"
        try:
            with open(tmp_path, "w", encoding="utf-8") as f:
                f.write(markdown)
            os.rename(tmp_path, out_path)
        except Exception as e:
            print(f"ERROR: Failed to write to {out_path}: {e}", file=sys.stderr)
            sys.exit(2)
    else:
        print(markdown)

if __name__ == "__main__":
    main(get_args())
