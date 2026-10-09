import argparse
import subprocess
import os
import sys
import json
import re

def main():
    parser = argparse.ArgumentParser(description="Delivery scope check")
    parser.add_argument('--base', required=True, help="Base SHA")
    parser.add_argument('--head', required=True, help="Head SHA")
    parser.add_argument('--files', required=True, help="Declared files, comma separated")
    parser.add_argument('--include-worktree', action='store_true', help="Include staged and untracked files")
    parser.add_argument('--json', action='store_true', default=True, help="Output in JSON format (default)")

    args = parser.parse_args()

    declared_files = set(f.strip() for f in args.files.split(',') if f.strip())

    # 1. Compute changed paths
    changed_paths = set()
    try:
        diff_out = subprocess.check_output(["git", "diff", "--name-only", args.base, args.head], text=True)
        for line in diff_out.splitlines():
            if line: changed_paths.add(line)
    except subprocess.CalledProcessError as e:
        sys.stderr.write(f"git diff failed: {e}\n")
        sys.exit(1)

    if args.include_worktree:
        try:
            diff_staged = subprocess.check_output(["git", "diff", "--name-only", args.head], text=True)
            for line in diff_staged.splitlines():
                if line: changed_paths.add(line)

            untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], text=True)
            for line in untracked.splitlines():
                if line: changed_paths.add(line)
        except subprocess.CalledProcessError as e:
            sys.stderr.write(f"git diff/ls-files failed: {e}\n")
            sys.exit(1)

    errors = []

    repo_root = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()

    total_files = len(changed_paths)
    if total_files > 4:
        errors.append(f"Too many files changed: {total_files} (max 4)")

    total_lines_changed = 0

    # Parse numstat
    def parse_numstat(output):
        nonlocal total_lines_changed
        for line in output.splitlines():
            parts = line.split('\t')
            if len(parts) == 3:
                adds, dels, path = parts
                if adds == '-' or dels == '-':
                    pass # binary
                else:
                    total_lines_changed += int(adds) + int(dels)

    numstat_base_head = subprocess.check_output(["git", "diff", "--numstat", args.base, args.head], text=True)
    parse_numstat(numstat_base_head)

    if args.include_worktree:
        numstat_staged = subprocess.check_output(["git", "diff", "--numstat", args.head], text=True)
        parse_numstat(numstat_staged)

        # Untracked files lines
        untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], text=True)
        for path in untracked.splitlines():
            if path:
                if os.path.exists(path):
                    if not os.path.islink(path):
                        try:
                            with open(path, 'r', encoding='utf-8') as f:
                                lines = f.readlines()
                                total_lines_changed += len(lines)
                        except UnicodeDecodeError:
                            pass # binary

    if total_lines_changed > 400:
        errors.append(f"Too many lines changed: {total_lines_changed} (max 400)")

    for path in changed_paths:
        if path not in declared_files:
            errors.append(f"Changed path not in declared list: {path}")

        if os.path.isabs(path):
            errors.append(f"Path is absolute: {path}")

        if ".." in path.split(os.sep):
            errors.append(f"Path contains '..' segment: {path}")

        # canonicalize
        abs_path = os.path.join(repo_root, path)
        if not os.path.lexists(abs_path):
            # The file might be deleted. If deleted, it's fine, it can't escape repo root or be broken symlink.
            pass
        elif not os.path.exists(abs_path):
            errors.append(f"Broken symlink: {path}")
        else:
            canonical = os.path.realpath(abs_path)
            if os.path.commonpath([canonical, repo_root]) != repo_root:
                errors.append(f"Path resolves outside repository: {path}")

        # Check if non-text file added
        # git diff tells us status if we use --diff-filter=A

    # Check added lines for absolute personal paths
    # We can get added lines using git diff -U0

    def check_added_lines(diff_lines, is_untracked=False):
        personal_path_regex = re.compile(r'(?:/home/|/Users/|(?:[A-Za-z]:[/\\])?[Uu]sers[/\\])[^/\\]+')
        for line in diff_lines:
            if is_untracked or line.startswith('+') and not line.startswith('+++'):
                if personal_path_regex.search(line):
                    return True
        return False

    has_personal_path = False

    diff_base_head = subprocess.check_output(["git", "diff", "-U0", args.base, args.head], text=True, errors='replace')
    if check_added_lines(diff_base_head.splitlines()):
        has_personal_path = True

    if args.include_worktree:
        diff_staged = subprocess.check_output(["git", "diff", "-U0", args.head], text=True, errors='replace')
        if check_added_lines(diff_staged.splitlines()):
            has_personal_path = True

        untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], text=True)
        for path in untracked.splitlines():
            if path and os.path.exists(path) and not os.path.isdir(path):
                try:
                    with open(path, 'r', encoding='utf-8') as f:
                        if check_added_lines(f.readlines(), is_untracked=True):
                            has_personal_path = True
                except UnicodeDecodeError:
                    pass

    if has_personal_path:
        errors.append("use $HOME/...")

    # Check if non-text file was added
    # Find all added files
    added_files = set()
    diff_name_status = subprocess.check_output(["git", "diff", "--name-status", args.base, args.head], text=True)
    for line in diff_name_status.splitlines():
        parts = line.split('\t', 1)
        if len(parts) == 2 and parts[0].startswith('A'):
            added_files.add(parts[1])

    if args.include_worktree:
        diff_name_status_staged = subprocess.check_output(["git", "diff", "--name-status", args.head], text=True)
        for line in diff_name_status_staged.splitlines():
            parts = line.split('\t', 1)
            if len(parts) == 2 and parts[0].startswith('A'):
                added_files.add(parts[1])

        untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], text=True)
        for line in untracked.splitlines():
            if line: added_files.add(line)

    # Check if added file is binary
    for path in added_files:
        if path not in changed_paths:
            continue
        # We can check using file command or numstat output
        # If it was in numstat with '-' it is binary
        is_binary = False
        # Look in numstat
        numstat_all = subprocess.check_output(["git", "diff", "--numstat", args.base, args.head], text=True)
        if args.include_worktree:
            numstat_all += "\n" + subprocess.check_output(["git", "diff", "--numstat", args.head], text=True)

        for line in numstat_all.splitlines():
            parts = line.split('\t')
            if len(parts) == 3:
                adds, dels, p = parts
                if p == path and (adds == '-' or dels == '-'):
                    is_binary = True
                    break

        # If untracked, we checked before by trying to read it
        if args.include_worktree and path in untracked.splitlines():
            if os.path.exists(path) and not os.path.isdir(path):
                try:
                    with open(path, 'r', encoding='utf-8') as f:
                        f.read()
                except UnicodeDecodeError:
                    is_binary = True

        if is_binary:
            errors.append(f"Non-text file added: {path}")

    verdict = "PASS" if not errors else "FAIL"

    report = {
        "head_sha": args.head,
        "base_sha": args.base,
        "changed_paths": sorted(list(changed_paths)),
        "counts": {
            "files": total_files,
            "lines_changed": total_lines_changed
        },
        "verdict": verdict
    }

    print(json.dumps(report, indent=2))

    if errors:
        for error in errors:
            sys.stderr.write(error + "\n")
        sys.exit(1)

if __name__ == '__main__':
    main()
