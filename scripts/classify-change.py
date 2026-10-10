#!/usr/bin/env python3
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

# Empty list means every actor is treated as an agent
HUMAN_ALLOWLIST = []

def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base")
    parser.add_argument("--head")
    parser.add_argument("--paths", nargs="*")
    parser.add_argument("--lines", type=int)
    parser.add_argument("--actor")
    parser.add_argument("--label")
    return parser.parse_args()

def normalize_path(path):
    if not path or os.path.isabs(path) or ".." in path.split(os.sep):
        return None
    return os.path.normpath(path)

def is_explicit_ask_path(path):
    if path is None:
        return True

    sensitive_prefixes = [
        "src/security/", "src/domain/security/", "src/crypto/",
        "src/keystore/", "src/auth2/", ".cargo/", ".env",
        "docs/", "panel-ui/", ".gitcore/", ".github/", ".husky/"
    ]

    for prefix in sensitive_prefixes:
        if path.startswith(prefix) or path.startswith(os.path.normpath(prefix)):
            return True

    if os.path.basename(path) in ["Cargo.toml", "Cargo.lock", "build.rs", ".gitleaks.toml"]:
        return True

    if path in ["package.json", "pnpm-workspace.yaml", "deny.toml", "rust-toolchain.toml", "clippy.toml"]:
        return True

    if path in ["scripts/check-secrets.sh", "scripts/verify-pipeline.sh"] or path.startswith("scripts/") or path.startswith("scripts" + os.sep):
        return True

    if "schema" in path.lower() or "migration" in path.lower():
        return True

    return False

def is_show_path(path):
    if not path:
        return False
    return path.startswith("src/") or path.startswith("crates/") or path.startswith("code-graph/")

def get_git_diff_stats(base, head):
    try:
        status_res = subprocess.run(
            ["git", "diff", "--name-status", "-z", "-M", f"{base}...{head}"],
            capture_output=True, text=True, check=True
        ).stdout
        numstat_res = subprocess.run(
            ["git", "diff", "--numstat", "-z", "-M", f"{base}...{head}"],
            capture_output=True, text=True, check=True
        ).stdout

        if not status_res.strip():
            return [], 0, False, False

        status_parts = status_res.split("\0")
        numstat_parts = numstat_res.split("\0")
        paths = []
        has_delete_or_rename = False
        has_binary = False
        total_lines = 0

        i = 0
        while i < len(status_parts) - 1:
            status = status_parts[i]
            if not status:
                break
            if status.startswith("R"):
                paths.extend([status_parts[i+1], status_parts[i+2]])
                has_delete_or_rename = True
                i += 3
            else:
                paths.append(status_parts[i+1])
                if status.startswith("D"):
                    has_delete_or_rename = True
                i += 2

        j = 0
        while j < len(numstat_parts) - 1:
            if not numstat_parts[j]:
                break
            part = numstat_parts[j]
            if "\t" in part:
                tabs = part.split("\t")
                added, deleted = tabs[0], tabs[1]
                if added == "-" or deleted == "-":
                    has_binary = True
                else:
                    total_lines += int(added) + int(deleted)

                if len(tabs) == 3 and not tabs[2]:
                    j += 3
                else:
                    j += 1
            else:
                j += 1

        return paths, total_lines, has_delete_or_rename, has_binary

    except subprocess.CalledProcessError:
        return None, 0, False, False

def compute_classification(paths, lines, actor, label, has_delete_or_rename=False, has_binary=False, is_empty_diff=False):
    if is_empty_diff or not paths:
        return {"class": "flow:ask", "reason": "Empty diff, missing range, or zero paths defaults to Ask.", "label_downgrade_attempted": False}

    has_ask_path = False
    all_show_paths = True

    for p in paths:
        norm = normalize_path(p)
        if is_explicit_ask_path(norm):
            has_ask_path = True
            all_show_paths = False
        elif not is_show_path(norm):
            has_ask_path = True
            all_show_paths = False

    if has_ask_path:
        computed_class, reason = "flow:ask", "Contains Ask path."
    elif has_binary:
        computed_class, reason = "flow:ask", "Contains binary file changes."
    elif all_show_paths:
        computed_class, reason = "flow:show", "Changes limited to show paths."
    else:
        computed_class, reason = "flow:ask", "Paths could not be proved as Ship or Show."

    is_human = (actor in HUMAN_ALLOWLIST) if actor else False
    if not is_human:
        if len(paths) > 4 or lines > 400:
            if FLOW_SEVERITY["flow:ask"] > FLOW_SEVERITY.get(computed_class, 0):
                computed_class, reason = "flow:ask", "Agent change exceeds file/line limit."

    label_downgrade_attempted = False
    if label and label in FLOW_SEVERITY:
        label_sev = FLOW_SEVERITY[label]
        computed_sev = FLOW_SEVERITY.get(computed_class, 0)
        if label_sev > computed_sev:
            computed_class = label
            reason = f"Maintained stricter label {label}. " + reason
        elif label_sev < computed_sev:
            label_downgrade_attempted = True
            reason += f" (Blocked downgrade from {computed_class} to {label})."

    return {"class": computed_class, "reason": reason, "label_downgrade_attempted": label_downgrade_attempted}

def main():
    args = parse_args()
    paths, lines = [], 0
    has_delete_or_rename = False
    has_binary = False
    is_empty_diff = False

    has_paths_args = args.paths is not None
    has_lines_args = args.lines is not None
    has_git_args = bool(args.base) or bool(args.head)

    if (has_paths_args or has_lines_args) and has_git_args:
        is_empty_diff = True
    elif has_paths_args or has_lines_args:
        if not (has_paths_args and has_lines_args) or not args.paths:
            is_empty_diff = True
        else:
            paths, lines = args.paths, args.lines
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

    result = compute_classification(paths, lines, args.actor, args.label, has_delete_or_rename, has_binary, is_empty_diff)
    print(json.dumps(result))

if __name__ == "__main__":
    main()
