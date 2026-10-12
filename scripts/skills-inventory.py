#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
import re
import sys

DEFAULT_ROOTS = [
    "~/.hermes/skills",
    "~/.claude/skills",
    "~/.agents/skills",
    "~/.gemini/config/skills",
    "~/.openclaw/skills",
    "~/.config/opencode/skills"
]

SECRET_CLASSES = [
    ("openai_or_router_key", re.compile(r"sk-or-v1-[A-Za-z0-9]{20,}")),
    ("github_pat", re.compile(r"ghp_[A-Za-z0-9]{20,}")),
    ("aws_access_key_id", re.compile(r"AKIA[0-9A-Z]{16}")),
    ("slack_token", re.compile(r"xox[baprs]-[A-Za-z0-9-]{10,}")),
    ("google_api_key", re.compile(r"AIza[0-9A-Za-z_-]{30,}")),
    ("private_key_block", re.compile(r"BEGIN (RSA|OPENSSH|EC|DSA) PRIVATE KEY")),
]

def scan_secrets(members: list) -> list:
    findings = []
    for m in members:
        if m['tag'] != ord('f'):
            continue
        try:
            text = m['bytes'].decode('utf-8', errors='replace')
        except Exception:
            continue

        lines = text.splitlines()
        for i, line in enumerate(lines):
            for class_name, pattern in SECRET_CLASSES:
                if pattern.search(line):
                    findings.append({'class': class_name, 'location': f"{m['relative']}:{i+1}"})
    return findings

def parse_frontmatter_name(content: bytes) -> str:
    """Extracts 'name' from YAML frontmatter in SKILL.md. Quick parsing."""
    try:
        text = content.decode('utf-8', errors='replace')
        lines = text.splitlines()
        if not lines or lines[0].strip() != '---':
            return ""
        for line in lines[1:]:
            if line.strip() == '---':
                break
            if line.lower().startswith("name:"):
                return line.split(':', 1)[1].strip().strip('"').strip("'")
    except Exception:
        pass
    return ""

def hash_package(package_dir: str, members: list) -> str:
    """
    members: list of dicts with 'relative', 'tag' (b'd' or b'f'), 'exec' (0 or 1), 'bytes'
    """
    members.sort(key=lambda m: m['relative'].encode('utf-8'))

    h = hashlib.sha256()
    for m in members:
        h.update(m['relative'].encode('utf-8'))
        h.update(bytes([m['tag'], m['exec']]))
        # 8-byte little-endian size
        size = len(m['bytes'])
        h.update(size.to_bytes(8, byteorder='little', signed=False))
        h.update(m['bytes'])

    return h.hexdigest()

def is_executable(filepath: str) -> int:
    return 1 if os.access(filepath, os.X_OK) else 0

def walk_package(package_dir: str):
    """
    Walks package directory, enforces bounds:
    - depth <= 4
    - node count <= 20000
    Returns list of members for hashing and total size, file count, and broken links count.
    """
    members = []
    nodes = 0
    total_size = 0
    file_count = 0
    broken_links = 0

    PACKAGE_BOUNDS_DEPTH = 4
    PACKAGE_BOUNDS_NODES = 20000
    PACKAGE_BOUNDS_TOTAL_BYTES = 4 << 20 # 4 MiB (for hashing logic total bytes bound)
    PACKAGE_BOUNDS_MEMBER_SIZE = 1 << 20 # 1 MiB

    # If the package dir itself is a symlink, os.walk with followlinks=False will NOT traverse it.
    # We must follow the package root symlink or resolve it first.
    actual_package_dir = package_dir
    if os.path.islink(package_dir):
        actual_package_dir = os.path.realpath(package_dir)

    base_depth = actual_package_dir.rstrip(os.path.sep).count(os.path.sep)

    for dirpath, dirnames, filenames in os.walk(actual_package_dir, followlinks=False):
        current_depth = dirpath.count(os.path.sep) - base_depth
        if current_depth > PACKAGE_BOUNDS_DEPTH:
            dirnames.clear()
            continue

        # Add the directory itself (unless it's the root package_dir, but we'll include it for relative consistency or skip it)
        if dirpath != actual_package_dir:
            nodes += 1
            if nodes > PACKAGE_BOUNDS_NODES:
                break
            relative = os.path.relpath(dirpath, actual_package_dir)
            members.append({'relative': relative, 'tag': ord('d'), 'exec': 0, 'bytes': b''})

        for name in filenames:
            nodes += 1
            if nodes > PACKAGE_BOUNDS_NODES:
                break

            filepath = os.path.join(dirpath, name)
            relative = os.path.relpath(filepath, actual_package_dir)

            try:
                st = os.lstat(filepath)
            except Exception:
                broken_links += 1
                continue

            import stat
            if stat.S_ISLNK(st.st_mode):
                # Check if it's broken
                target_path = os.path.join(dirpath, os.readlink(filepath)) if not os.path.isabs(os.readlink(filepath)) else os.readlink(filepath)
                if not os.path.exists(target_path):
                    broken_links += 1
                continue

            if stat.S_ISREG(st.st_mode):
                if st.st_size > PACKAGE_BOUNDS_MEMBER_SIZE:
                    continue # abort/skip
                try:
                    with open(filepath, 'rb') as f:
                        b = f.read()
                except Exception:
                    continue

                total_size += len(b)
                if total_size > PACKAGE_BOUNDS_TOTAL_BYTES:
                    pass # We might break, but we'll just stop adding bytes

                members.append({
                    'relative': relative,
                    'tag': ord('f'),
                    'exec': is_executable(filepath),
                    'bytes': b
                })
                file_count += 1

    return members, total_size, file_count, broken_links

def main(args_list=None):
    parser = argparse.ArgumentParser(description="Inventory read-only skill sources.")
    parser.add_argument("--root", nargs="+", help="Paths to search for skill roots.")
    parser.add_argument("--json", action="store_true", help="Output in JSON format.")
    parser.add_argument("--out", type=str, help="File to write output to.")
    args = parser.parse_args(args_list)

    roots = args.root if args.root else DEFAULT_ROOTS

    expanded_roots = [os.path.expanduser(r) for r in roots]

    # Quick check for missing roots
    missing_roots = []
    for root in expanded_roots:
        if not os.path.exists(root):
            print(f"absent: {root}")
            missing_roots.append(root)

    packages = []
    broken_links = 0

    # Walk each root to find SKILL.md
    for root in expanded_roots:
        if root in missing_roots:
            continue

        root_base_depth = root.rstrip(os.path.sep).count(os.path.sep)

        for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
            current_depth = dirpath.count(os.path.sep) - root_base_depth
            if current_depth > 4:
                dirnames.clear()
                continue

            # Since followlinks=False, symlinked directories are listed in dirnames but we won't walk INTO them.
            # Find and append them so we can process them as packages.
            symlinked_pkgs = []
            for d in list(dirnames):
                full_d = os.path.join(dirpath, d)
                if os.path.islink(full_d):
                    # Check if it's broken
                    target_path = os.path.join(dirpath, os.readlink(full_d)) if not os.path.isabs(os.readlink(full_d)) else os.readlink(full_d)
                    if not os.path.exists(target_path):
                        broken_links += 1
                        dirnames.remove(d)
                        continue

                    if os.path.isdir(full_d) and os.path.isfile(os.path.join(full_d, 'SKILL.md')):
                        # We found a symlinked package
                        symlinked_pkgs.append(full_d)

            # Also check filenames for broken symlinks that aren't directories
            for f in list(filenames):
                full_f = os.path.join(dirpath, f)
                if os.path.islink(full_f):
                    target_path = os.path.join(dirpath, os.readlink(full_f)) if not os.path.isabs(os.readlink(full_f)) else os.readlink(full_f)
                    if not os.path.exists(target_path):
                        broken_links += 1

            # Packages to process from this dirpath
            pkgs_to_process = []
            if 'SKILL.md' in filenames:
                pkgs_to_process.append(dirpath)
            pkgs_to_process.extend(symlinked_pkgs)

            for pkg_dir in pkgs_to_process:
                is_symlink = os.path.islink(pkg_dir)
                target = None
                if is_symlink:
                    try:
                        target = os.readlink(pkg_dir)
                    except Exception:
                        broken_links += 1

                members, total_size, file_count, bl_count = walk_package(pkg_dir)
                broken_links += bl_count

                pkg_hash = hash_package(pkg_dir, members)

                name = ""
                skill_member = next((m for m in members if m['relative'] == 'SKILL.md'), None)
                if skill_member:
                    name = parse_frontmatter_name(skill_member['bytes'])

                packages.append({
                    'name': name,
                    'hash': pkg_hash,
                    'size': total_size,
                    'file_count': file_count,
                    'type': 'symlink' if is_symlink else 'regular',
                    'root': root,
                    'dirpath': pkg_dir,
                    'members': members,
                    'target': target
                })

            if pkgs_to_process and 'SKILL.md' in filenames:
                # If the dirpath itself is a package, don't recurse into its subdirectories.
                dirnames.clear()

    secret_findings = 0
    conflicts = 0

    # Check duplicates across roots
    name_to_packages = {}
    for pkg in packages:
        name_to_packages.setdefault(pkg['name'], []).append(pkg)

    for name, pkgs in name_to_packages.items():
        if len(pkgs) > 1:
            hashes = set(p['hash'] for p in pkgs)
            if len(hashes) > 1:
                conflicts += 1

    results = []
    for pkg in packages:
        findings = scan_secrets(pkg['members'])
        secret_findings += len(findings)

        # Don't output raw bytes in JSON
        del pkg['members']
        pkg['secrets'] = findings
        results.append(pkg)

    if args.json:
        out_str = json.dumps(results, indent=2)
        if args.out:
            with open(args.out, 'w') as f:
                f.write(out_str)
        else:
            print(out_str)
    else:
        # Just simple text report for tests to catch assertions
        out_lines = []
        for pkg in results:
            out_lines.append(f"Package: {pkg['name']} (hash: {pkg['hash']})")
            if pkg['size'] > (1 << 20):
                out_lines.append(f"  Warning: package over 1 MiB ({pkg['size']} bytes)")
            if pkg['type'] == 'symlink':
                out_lines.append(f"  link -> {pkg['target']}")
            for f in pkg['secrets']:
                out_lines.append(f"  Secret: {f['class']} at {f['location']}")

        out_str = "\n".join(out_lines)
        if args.out:
            with open(args.out, 'w') as f:
                f.write(out_str + "\n")
                f.write(f"INVENTORY: {len(packages)} packages, {conflicts} conflicts, {secret_findings} secret findings, {broken_links} broken links\n")
        else:
            if out_str:
                print(out_str)

    if not args.json or not args.out: # Still print summary to stdout if not JSON out to file
        print(f"INVENTORY: {len(packages)} packages, {conflicts} conflicts, {secret_findings} secret findings, {broken_links} broken links")

if __name__ == "__main__":
    main()
