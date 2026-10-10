#!/usr/bin/env python3
import argparse
import json
import subprocess
import sys
import os

def get_git_diff(base, head):
    try:
        output = subprocess.check_output(
            ['git', 'diff', '--name-status', f'{base}...{head}'],
            stderr=subprocess.STDOUT,
            text=True
        )
        changed_files = set()
        for line in output.splitlines():
            parts = line.strip().split('\t')
            if not parts:
                continue
            status = parts[0]
            if status.startswith('R') or status.startswith('C'):
                if len(parts) >= 3:
                    changed_files.add(parts[1])
                    changed_files.add(parts[2])
            else:
                if len(parts) >= 2:
                    changed_files.add(parts[1])
        return list(changed_files)
    except subprocess.CalledProcessError as e:
        sys.exit(f"Failed to get git diff: {e.output}")

def get_cargo_metadata():
    try:
        cmd = ['cargo', 'metadata', '--format-version', '1', '--all-features']
        if os.environ.get('CI'):
            cmd.append('--locked')
        output = subprocess.check_output(cmd, stderr=subprocess.STDOUT, text=True)
        return json.loads(output)
    except subprocess.CalledProcessError as e:
        sys.exit(f"Failed to run cargo metadata: {e.output}")

def build_package_graph(metadata, repo_root):
    if not metadata.get('workspace_members'):
        sys.stderr.write("Error: empty workspace_members\n")
        sys.exit(1)
    if 'resolve' not in metadata or not metadata['resolve'].get('nodes'):
        sys.stderr.write("Error: missing resolve or nodes in metadata\n")
        sys.exit(1)

    workspace_members = set(metadata['workspace_members'])

    packages = {}
    for pkg in metadata['packages']:
        if pkg['id'] in workspace_members:
            pkg_dir = os.path.dirname(pkg['manifest_path'])
            packages[pkg['id']] = {
                'name': pkg['name'],
                'dir': pkg_dir,
                'reverse_deps': set(),
                'targets': pkg.get('targets', [])
            }

    # Build reverse dependencies
    for node in metadata['resolve']['nodes']:
        if node['id'] in packages:
            for dep in node['deps']:
                dep_id = dep['pkg']
                if dep_id in packages:
                    packages[dep_id]['reverse_deps'].add(node['id'])

    # Explicit edges
    code_graph_id = None
    xavier_id = None
    for pid, p in packages.items():
        if p['name'] == 'code-graph':
            code_graph_id = pid
        elif p['name'] == 'xavier':
            xavier_id = pid

    if code_graph_id and xavier_id:
        packages[code_graph_id]['reverse_deps'].add(xavier_id)

    for pid, p in packages.items():
        if 'code-graph/parsers/' in p['dir'] or p['name'].startswith('parser-'):
            if code_graph_id:
                p['reverse_deps'].add(code_graph_id)
            if xavier_id:
                p['reverse_deps'].add(xavier_id)

    return packages

def resolve_repo_root():
    try:
        out = subprocess.check_output(['git', 'rev-parse', '--show-toplevel'], text=True)
        return out.strip()
    except:
        return os.getcwd()

def check_file_impact(filepath, packages, repo_root):
    # Returns (pkg_id, is_fallback, fallback_reason, affected_target_names)

    if filepath.startswith('panel-ui/src-tauri/'):
        return ('app', False, None, ['app'])

    parts = filepath.split('/')
    filename = parts[-1]

    if filename in ('Cargo.toml', 'Cargo.lock', 'build.rs', 'rust-toolchain', 'rust-toolchain.toml') \
       or '.cargo' in parts or 'vendor' in parts:
        return (None, True, f"Modified sensitive configuration: {filepath}", [])

    if filepath.startswith('.github/') or '/.github/' in filepath:
        return (None, True, f"Modified CI file: {filepath}", [])

    abs_path = os.path.abspath(os.path.join(repo_root, filepath))

    best_pkg_id = None
    best_len = -1

    for pkg_id, pkg_info in packages.items():
        if abs_path.startswith(pkg_info['dir'] + os.sep) or abs_path == pkg_info['dir']:
            if pkg_info['dir'] == repo_root:
                rel_path = os.path.relpath(abs_path, repo_root)
                root_parts = rel_path.split(os.sep)
                if root_parts[0] not in ('src', 'tests', 'benches', 'examples'):
                    continue

            if len(pkg_info['dir']) > best_len:
                best_len = len(pkg_info['dir'])
                best_pkg_id = pkg_id

    if best_pkg_id is None:
        return (None, True, f"Unknown path outside recognized rust package directories: {filepath}", [])

    if not filepath.endswith('.rs'):
        return (None, True, f"Modified non-.rs file inside a member: {filepath}", [])

    pkg_info = packages[best_pkg_id]
    affected_targets = []

    in_tests = ('/tests/' in abs_path) or ('/benches/' in abs_path) or ('/examples/' in abs_path)
    if in_tests:
        matched = False
        for t in pkg_info['targets']:
            if os.path.abspath(t['src_path']) == abs_path:
                affected_targets.append(t['name'])
                matched = True
        if not matched:
            for t in pkg_info['targets']:
                if 'test' in t['kind'] or 'bench' in t['kind']:
                    affected_targets.append(t['name'])
    else:
        for t in pkg_info['targets']:
            affected_targets.append(t['name'])

    return (best_pkg_id, False, None, affected_targets)

def main():
    parser = argparse.ArgumentParser(description="Calculate Rust test impact based on changed files.")
    parser.add_argument('--base', help="Base git ref")
    parser.add_argument('--head', help="Head git ref (defaults to HEAD if --base is provided)")
    parser.add_argument('--paths', nargs='*', help="List of changed file paths")
    parser.add_argument('--mock-metadata', help="Path to mock metadata JSON (for testing)")

    args = parser.parse_args()

    if args.paths is not None:
        changed_files = args.paths
    elif args.base is not None:
        head = args.head if args.head else 'HEAD'
        changed_files = get_git_diff(args.base, head)
    else:
        parser.error("Must provide either --paths or --base")

    repo_root = resolve_repo_root()

    if args.mock_metadata:
        with open(args.mock_metadata, 'r') as f:
            metadata = json.load(f)
    else:
        metadata = get_cargo_metadata()

    packages = build_package_graph(metadata, repo_root)

    fallback = False
    reason = None
    modified_pkgs = {}

    for filepath in changed_files:
        pkg_id, is_fallback, fb_reason, affected_targets = check_file_impact(filepath, packages, repo_root)
        if is_fallback:
            fallback = True
            reason = fb_reason
            break
        if pkg_id:
            if pkg_id not in modified_pkgs:
                modified_pkgs[pkg_id] = set()
            modified_pkgs[pkg_id].update(affected_targets)

    if fallback:
        all_pkgs = []
        for p in packages.values():
            targets = [t['name'] for t in p['targets']]
            if targets:
                all_pkgs.append({
                    "name": p['name'],
                    "targets": sorted(targets)
                })
        if not all_pkgs:
            sys.stderr.write("Error: Zero fallback packages selected.\n")
            sys.exit(1)
        result = {
            "fallback": True,
            "reason": reason,
            "packages": sorted(all_pkgs, key=lambda x: x['name'])
        }
        print(json.dumps(result, indent=2))
        return

    selected_ids = set(modified_pkgs.keys())
    queue = list(modified_pkgs.keys())

    while queue:
        current = queue.pop(0)
        if current == 'app':
            continue

        for rev_dep in packages[current]['reverse_deps']:
            if rev_dep not in selected_ids:
                selected_ids.add(rev_dep)
                queue.append(rev_dep)
                if rev_dep not in modified_pkgs:
                    modified_pkgs[rev_dep] = set()
                modified_pkgs[rev_dep].update(t['name'] for t in packages[rev_dep]['targets'])

    selected_packages = []
    for pid in selected_ids:
        if pid == 'app':
            selected_packages.append({
                "name": "app",
                "targets": ["app"]
            })
            continue

        pkg = packages[pid]
        targets = list(modified_pkgs.get(pid, set()))
        if not targets:
            sys.stderr.write(f"Error: Selected package {pkg['name']} has no test target.\n")
            sys.exit(1)

        selected_packages.append({
            "name": pkg['name'],
            "targets": sorted(targets)
        })

    if not selected_packages:
        sys.stderr.write("Error: Zero selected-test result fails.\n")
        sys.exit(1)

    result = {
        "fallback": False,
        "reason": "Scoped selection based on changed files",
        "packages": sorted(selected_packages, key=lambda x: x['name'])
    }
    print(json.dumps(result, indent=2))

if __name__ == "__main__":
    main()
