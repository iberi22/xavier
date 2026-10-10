#!/usr/bin/env python3
"""
classify-change.py

Implements path and actor classification for PRs to determine Ship/Show/Ask flow.
Outputs a fail-closed computed class and reasoning in JSON format.
"""

import argparse
import json
import os
import subprocess
import sys

# Severity hierarchy for labels/flow. Higher value = more restrictive.
FLOW_SEVERITY = {
    "flow:ship": 1,
    "flow:show": 2,
    "flow:ask": 3
}

# Paths that are sensitive or unknown and should default to Ask.
SENSITIVE_PREFIXES = [
    ".github/workflows/",
    ".gitcore/",
    ".env",
    "data/",
]

# Allowed prefixes (directories known to be somewhat safe). Anything else is "unknown"
ALLOWED_PREFIXES = [
    "src/",
    "scripts/",
    "tests/",
    "docs/",
    "panel-ui/",
    "public/",
    "installer/",
    "skills/",
    "plugins/",
    "crates/",
    ".github/" # but workflows are sensitive
]

def parse_args():
    parser = argparse.ArgumentParser(description="Compute PR classification for Ship/Show/Ask flow")
    parser.add_argument("--base", help="Base git ref")
    parser.add_argument("--head", help="Head git ref")
    parser.add_argument("--paths", nargs="*", help="List of modified paths (overrides git diff)")
    parser.add_argument("--lines", type=int, help="Total number of changed lines (overrides git diff)")
    parser.add_argument("--actor", help="Actor proposing the change (e.g., 'agent')")
    parser.add_argument("--label", help="Existing label on the PR (e.g., 'flow:ship')")
    return parser.parse_args()

def get_git_diff_stats(base, head):
    try:
        result = subprocess.run(
            ["git", "diff", "--numstat", base, head],
            capture_output=True,
            text=True,
            check=True
        )
        paths = []
        total_lines = 0
        for line in result.stdout.strip().split("\n"):
            if not line:
                continue
            parts = line.split("\t")
            if len(parts) == 3:
                added, deleted, path = parts
                paths.append(path)
                if added != "-":
                    total_lines += int(added)
                if deleted != "-":
                    total_lines += int(deleted)
        return paths, total_lines
    except subprocess.CalledProcessError as e:
        print(f"Error running git diff: {e}", file=sys.stderr)
        sys.exit(1)

def is_path_sensitive_or_unknown(path):
    for sensitive in SENSITIVE_PREFIXES:
        if path.startswith(sensitive):
            return True, f"Sensitive path modified: {path}"

    is_known = False
    for allowed in ALLOWED_PREFIXES:
        if path.startswith(allowed):
            is_known = True
            break

    # Also allow root level files like README.md, Cargo.toml, etc.
    if "/" not in path:
        is_known = True

    if not is_known:
        return True, f"Unknown path modified: {path}"

    return False, ""

def compute_classification(paths, lines, actor, label):
    computed_class = "flow:ship"
    reason = "Change meets ship criteria."

    # 1. Path Classification
    for path in paths:
        sensitive, msg = is_path_sensitive_or_unknown(path)
        if sensitive:
            computed_class = "flow:ask"
            reason = msg
            break

    # 2. Actor Classification (Agent limits)
    # Agent Ship is limited to four files and 400 changed lines
    if "agent" in str(actor).lower():
        if len(paths) > 4:
            if FLOW_SEVERITY["flow:ask"] > FLOW_SEVERITY.get(computed_class, 0):
                computed_class = "flow:ask"
                reason = f"Agent change exceeds file limit (files={len(paths)}, max=4)."
        if lines > 400:
            if FLOW_SEVERITY["flow:ask"] > FLOW_SEVERITY.get(computed_class, 0):
                computed_class = "flow:ask"
                reason = f"Agent change exceeds line limit (lines={lines}, max=400)."

    # 3. Label Downgrade Prevention
    if label and label in FLOW_SEVERITY:
        label_sev = FLOW_SEVERITY[label]
        computed_sev = FLOW_SEVERITY.get(computed_class, 0)

        if label_sev > computed_sev:
            # PR already has a more restrictive label, keep it
            computed_class = label
            reason = f"Maintained existing restrictive label {label}."
        elif label_sev < computed_sev:
            # Trying to downgrade (e.g. computed is ask, but label is ship)
            reason += f" (Blocked label downgrade from {computed_class} to {label})."

    return {
        "class": computed_class,
        "reason": reason
    }

def main():
    args = parse_args()

    paths = []
    lines = 0

    if args.paths is not None or args.lines is not None:
        if args.paths is not None:
            paths = args.paths
        if args.lines is not None:
            lines = args.lines
    elif args.base and args.head:
        paths, lines = get_git_diff_stats(args.base, args.head)
    else:
        # Default fail-closed if insufficient info
        print(json.dumps({
            "class": "flow:ask",
            "reason": "Missing required arguments for diff or explicit paths/lines."
        }))
        sys.exit(0)

    result = compute_classification(paths, lines, args.actor, args.label)
    print(json.dumps(result))

if __name__ == "__main__":
    main()
