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

FLOW_SEVERITY = {
    "flow:ship": 1,
    "flow:show": 2,
    "flow:ask": 3
}

HUMAN_ALLOWLIST = []

def parse_args():
    parser = argparse.ArgumentParser(description="Compute PR classification for Ship/Show/Ask flow")
    parser.add_argument("--base", help="Base git ref")
    parser.add_argument("--head", help="Head git ref")
    parser.add_argument("--paths", nargs="*", help="List of modified paths (overrides git diff)")
    parser.add_argument("--lines", type=int, help="Total number of changed lines (overrides git diff)")
    parser.add_argument("--actor", help="Actor proposing the change (e.g., 'agent')")
    parser.add_argument("--label", help="Existing label on the PR (e.g., 'flow:ship')")
    return parser.parse_args()

def normalize_path(path):
    if not path:
        return None

    # Check for absolute path or symlink escape
    if os.path.isabs(path) or ".." in path.split(os.sep):
        return None

    return os.path.normpath(path)

def is_explicit_ask_path(path):
    if path is None:
        return True # un-normalized or absolute -> Ask

    # Check sensitive prefixes
    sensitive_prefixes = [
        "src/security/",
        "src/domain/security/",
        "src/crypto/",
        "src/keystore/",
        "src/auth2/",
        ".cargo/",
        ".env", # catches .env.example
        "docs/",
        "panel-ui/",
        ".gitcore/",
        ".github/",
        ".husky/"
    ]

    for prefix in sensitive_prefixes:
        if path.startswith(prefix) or path.startswith(os.path.normpath(prefix)):
            return True

    # Base names
    basename = os.path.basename(path)
    if basename in ["Cargo.toml", "Cargo.lock", "build.rs", ".gitleaks.toml"]:
        return True

    # Root configs
    if path in ["package.json", "pnpm-workspace.yaml", "deny.toml", "rust-toolchain.toml", "clippy.toml"]:
        return True

    # Scripts that are CI gates
    if path in ["scripts/check-secrets.sh", "scripts/verify-pipeline.sh"]:
        return True

    # Any other script is ambiguous -> Ask
    if path.startswith("scripts/") or path.startswith("scripts" + os.sep):
        return True

    # Schema and migration paths
    if "schema" in path.lower() or "migration" in path.lower():
        return True

    return False

def is_show_path(path):
    if not path:
        return False
    if path.startswith("src/") or path.startswith("crates/") or path.startswith("code-graph/"):
        return True
    return False

def get_git_diff_stats(base, head):
    try:
        status_res = subprocess.run(
            ["git", "diff", "--name-status", "-z", "-M", f"{base}...{head}"],
            capture_output=True, text=True, check=True
        )
        numstat_res = subprocess.run(
            ["git", "diff", "--numstat", "-z", "-M", f"{base}...{head}"],
            capture_output=True, text=True, check=True
        )

        status_parts = status_res.stdout.split("\0")
        numstat_parts = numstat_res.stdout.split("\0")

        if not status_res.stdout.strip():
            # Empty diff
            return [], 0, False, False

        paths = []
        has_delete_or_rename = False

        i = 0
        while i < len(status_parts) - 1:
            status = status_parts[i]
            if not status:
                break
            if status.startswith("R"):
                old_path = status_parts[i+1]
                new_path = status_parts[i+2]
                paths.extend([old_path, new_path])
                has_delete_or_rename = True
                i += 3
            else:
                path = status_parts[i+1]
                paths.append(path)
                if status.startswith("D"):
                    has_delete_or_rename = True
                i += 2

        # Parse numstat
        j = 0
        total_lines = 0
        has_binary = False
        while j < len(numstat_parts) - 1:
            if not numstat_parts[j]:
                break
            part = numstat_parts[j]
            if "\t" in part:
                tabs = part.split("\t")
                added = tabs[0]
                deleted = tabs[1]
                if added == "-" or deleted == "-":
                    has_binary = True
                else:
                    total_lines += int(added) + int(deleted)

                if len(tabs) == 3 and not tabs[2]:
                    # Rename case (added \t deleted \t \0 old \0 new \0)
                    j += 3 # skip the next two path parts
                else:
                    j += 1
            else:
                j += 1

        return paths, total_lines, has_delete_or_rename, has_binary

    except subprocess.CalledProcessError as e:
        return None, 0, False, False

def compute_classification(paths, lines, actor, label, has_delete_or_rename=False, has_binary=False, is_empty_diff=False):
    computed_class = "flow:ask"
    reason = "Defaulted to Ask."

    if is_empty_diff or not paths:
        return {
            "class": "flow:ask",
            "reason": "Empty diff, missing range, or zero paths defaults to Ask.",
            "label_downgrade_attempted": False
        }

    # Normalize paths and check for Ask
    normalized_paths = []
    has_ask_path = False
    all_show_paths = True

    for p in paths:
        norm = normalize_path(p)
        normalized_paths.append(norm)

        if is_explicit_ask_path(norm):
            has_ask_path = True
            all_show_paths = False
        elif not is_show_path(norm):
            # Unknown paths -> Ask
            has_ask_path = True
            all_show_paths = False

    if has_ask_path:
        computed_class = "flow:ask"
        reason = "Contains Ask path (sensitive, unknown, un-normalized, or script)."
    elif has_binary:
        computed_class = "flow:ask"
        reason = "Contains binary file changes."
    elif all_show_paths:
        computed_class = "flow:show"
        reason = "Changes limited to show paths (src, crates, code-graph)."
    else:
        # Fallback
        computed_class = "flow:ask"
        reason = "Paths could not be proved as Ship or Show, defaulting to Ask."

    # Actor evaluation
    is_human = (actor in HUMAN_ALLOWLIST) if actor else False
    if not is_human:
        # Agent
        if len(paths) > 4:
            if FLOW_SEVERITY["flow:ask"] > FLOW_SEVERITY.get(computed_class, 0):
                computed_class = "flow:ask"
                reason = f"Agent change exceeds file limit (files={len(paths)}, max=4)."
        if lines > 400:
            if FLOW_SEVERITY["flow:ask"] > FLOW_SEVERITY.get(computed_class, 0):
                computed_class = "flow:ask"
                reason = f"Agent change exceeds line limit (lines={lines}, max=400)."

    # Label Evaluation
    label_downgrade_attempted = False
    if label and label in FLOW_SEVERITY:
        label_sev = FLOW_SEVERITY[label]
        computed_sev = FLOW_SEVERITY.get(computed_class, 0)

        if label_sev > computed_sev:
            # PR already has a more restrictive label, keep it
            computed_class = label
            reason = f"Maintained existing stricter label {label}. " + reason
        elif label_sev < computed_sev:
            label_downgrade_attempted = True
            reason += f" (Blocked label downgrade from {computed_class} to {label})."

    return {
        "class": computed_class,
        "reason": reason,
        "label_downgrade_attempted": label_downgrade_attempted
    }

def main():
    args = parse_args()

    paths = []
    lines = 0
    has_delete_or_rename = False
    has_binary = False
    is_empty_diff = False

    # CLI checking
    has_paths_args = args.paths is not None
    has_lines_args = args.lines is not None
    has_git_args = bool(args.base) or bool(args.head)

    if (has_paths_args or has_lines_args) and has_git_args:
        # Both modes at once
        is_empty_diff = True
    elif has_paths_args or has_lines_args:
        if not (has_paths_args and has_lines_args):
            # Missing one of them
            is_empty_diff = True
        elif len(args.paths) == 0:
            is_empty_diff = True
        else:
            paths = args.paths
            lines = args.lines
    elif args.base and args.head:
        result = get_git_diff_stats(args.base, args.head)
        if result[0] is None:
            is_empty_diff = True
        else:
            paths, lines, has_delete_or_rename, has_binary = result
            if not paths:
                is_empty_diff = True
    else:
        is_empty_diff = True

    result = compute_classification(
        paths, lines, args.actor, args.label,
        has_delete_or_rename=has_delete_or_rename,
        has_binary=has_binary,
        is_empty_diff=is_empty_diff
    )

    print(json.dumps(result))

if __name__ == "__main__":
    main()
