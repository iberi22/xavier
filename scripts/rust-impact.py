#!/usr/bin/env python3
import argparse
import json
import subprocess
import sys
import os

def get_git_diff(base, head):
    try:
        output = subprocess.check_output(
            ['git', 'diff', '--name-only', f'{base}...{head}'],
            stderr=subprocess.STDOUT,
            text=True
        )
        return [line.strip() for line in output.splitlines() if line.strip()]
    except subprocess.CalledProcessError as e:
        sys.exit(f"Failed to get git diff: {e.output}")

def get_cargo_metadata():
    try:
        output = subprocess.check_output(
            ['cargo', 'metadata', '--format-version', '1'],
            stderr=subprocess.STDOUT,
            text=True
        )
        return json.loads(output)
    except subprocess.CalledProcessError as e:
        sys.exit(f"Failed to run cargo metadata: {e.output}")

def build_package_graph(metadata, repo_root):
    workspace_members = set(metadata['workspace_members'])

    # Map package ID to its info
    packages = {}
    for pkg in metadata['packages']:
        if pkg['id'] in workspace_members:
            pkg_dir = os.path.dirname(pkg['manifest_path'])
            packages[pkg['id']] = {
                'name': pkg['name'],
                'dir': pkg_dir,
                'reverse_deps': set()
            }

    # Build reverse dependencies
    if 'resolve' in metadata and 'nodes' in metadata['resolve']:
        for node in metadata['resolve']['nodes']:
            if node['id'] in packages:
                for dep in node['deps']:
                    dep_id = dep['pkg']
                    if dep_id in packages:
                        packages[dep_id]['reverse_deps'].add(node['id'])

    return packages

def resolve_repo_root():
    try:
        out = subprocess.check_output(['git', 'rev-parse', '--show-toplevel'], text=True)
        return out.strip()
    except:
        return os.getcwd()

def find_owning_package(filepath, packages, repo_root):
    abs_path = os.path.abspath(os.path.join(repo_root, filepath))

    # We want to map a file to a rust package.
    # However, if it's the root package (e.g. xavier), its dir is the repo root.
    # We should only consider it a match if it's inside src/, tests/, benches/, or examples/
    # of the root package, OR if it's in a subdirectory package (like crates/...).

    best_pkg_id = None
    best_len = -1

    for pkg_id, pkg_info in packages.items():
        if abs_path.startswith(pkg_info['dir'] + os.sep) or abs_path == pkg_info['dir']:
            # If the package is at repo root, only match if it's inside specific Rust source folders
            if pkg_info['dir'] == repo_root:
                rel_path = os.path.relpath(abs_path, repo_root)
                # Allow recognized root rust directories
                parts = rel_path.split(os.sep)
                if parts[0] not in ('src', 'tests', 'benches', 'examples', 'Cargo.toml', 'Cargo.lock'):
                    continue

            if len(pkg_info['dir']) > best_len:
                best_len = len(pkg_info['dir'])
                best_pkg_id = pkg_id

    return best_pkg_id

def main():
    parser = argparse.ArgumentParser(description="Calculate Rust test impact based on changed files.")
    parser.add_argument('--base', help="Base git ref")
    parser.add_argument('--head', help="Head git ref (defaults to HEAD if --base is provided)")
    parser.add_argument('--paths', nargs='*', help="List of changed file paths")

    args = parser.parse_args()

    if args.paths is not None:
        changed_files = args.paths
    elif args.base is not None:
        head = args.head if args.head else 'HEAD'
        changed_files = get_git_diff(args.base, head)
    else:
        parser.error("Must provide either --paths or --base")

    repo_root = resolve_repo_root()
    metadata = get_cargo_metadata()
    packages = build_package_graph(metadata, repo_root)

    fallback = False
    reason = None
    modified_pkg_ids = set()

    for filepath in changed_files:
        # Check fail-closed conditions
        if os.path.basename(filepath) == 'Cargo.toml' or os.path.basename(filepath) == 'Cargo.lock':
            fallback = True
            reason = f"Modified manifest/lockfile: {filepath}"
            break

        if filepath.startswith('.github/') or '/.github/' in filepath:
            fallback = True
            reason = f"Modified CI file: {filepath}"
            break

        pkg_id = find_owning_package(filepath, packages, repo_root)
        if pkg_id is None:
            fallback = True
            reason = f"Unknown path outside recognized rust package directories: {filepath}"
            break

        modified_pkg_ids.add(pkg_id)

    if fallback:
        result = {
            "fallback": True,
            "reason": reason,
            "packages": sorted([pkg['name'] for pkg in packages.values()])
        }
    else:
        # Calculate reverse dependencies
        selected_ids = set(modified_pkg_ids)
        queue = list(modified_pkg_ids)
        while queue:
            current = queue.pop(0)
            for rev_dep in packages[current]['reverse_deps']:
                if rev_dep not in selected_ids:
                    selected_ids.add(rev_dep)
                    queue.append(rev_dep)

        selected_packages = [packages[pid]['name'] for pid in selected_ids]

        if not selected_packages:
            sys.stderr.write("Error: Zero selected-test result fails.\n")
            sys.exit(1)

        result = {
            "fallback": False,
            "reason": "Scoped selection based on changed files",
            "packages": sorted(selected_packages)
        }

    print(json.dumps(result, indent=2))

if __name__ == "__main__":
    main()
