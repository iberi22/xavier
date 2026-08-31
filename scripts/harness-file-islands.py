#!/usr/bin/env python3
"""
Harness File Islands Verifier for Xavier (Wave-1 Foundation)

Verifies that the file islands configured for wave-1 are strictly disjoint
(zero overlapping files between parallel task islands).
"""

import sys
import os
import glob
from pathlib import Path

# Definition of Wave-1 10 disjoint file islands
WAVE_1_ISLANDS = [
    {
        "id": "island-1",
        "name": "Inbound Ports",
        "paths": ["src/ports/inbound/"],
    },
    {
        "id": "island-2",
        "name": "Outbound Ports",
        "paths": ["src/ports/outbound/"],
    },
    {
        "id": "island-3",
        "name": "Memory Engine",
        "paths": ["src/memory/"],
    },
    {
        "id": "island-4",
        "name": "Embedding Subsystem",
        "paths": ["src/embedding/"],
    },
    {
        "id": "island-5",
        "name": "Security & Redaction",
        "paths": ["src/security/"],
    },
    {
        "id": "island-6",
        "name": "Server & MCP Routes",
        "paths": ["src/server/"],
    },
    {
        "id": "island-7",
        "name": "CLI Handlers",
        "paths": ["src/cli/"],
    },
    {
        "id": "island-8",
        "name": "BYO Nodes & Vault",
        "paths": ["src/nodes/"],
    },
    {
        "id": "island-9",
        "name": "P2P Mesh Network",
        "paths": ["src/mesh/"],
    },
    {
        "id": "island-10",
        "name": "Docs & Harness Infrastructure",
        "paths": ["docs/", "scripts/"],
    },
]


def expand_island_files(repo_root, island):
    """
    Expands directory/file paths for an island into a set of normalized relative file paths.
    """
    file_set = set()
    for path_str in island["paths"]:
        target = repo_root / path_str
        if target.is_file():
            rel = target.relative_to(repo_root).as_posix()
            file_set.add(rel)
        elif target.is_dir():
            for p in target.rglob("*"):
                if p.is_file():
                    rel = p.relative_to(repo_root).as_posix()
                    file_set.add(rel)
        else:
            # Glob pattern expansion
            matched = glob.glob(str(target), recursive=True)
            for m in matched:
                p = Path(m)
                if p.is_file():
                    rel = p.relative_to(repo_root).as_posix()
                    file_set.add(rel)
    return file_set


def verify_wave_1(repo_root):
    print(f"=== Harness File Islands: Verifying Wave-1 (10 islands) ===")

    island_files = {}
    for island in WAVE_1_ISLANDS:
        files = expand_island_files(repo_root, island)
        island_files[island["id"]] = {
            "name": island["name"],
            "files": files
        }
        print(f"  - [{island['id']}] {island['name']}: {len(files)} archivos")

    conflicts = []
    island_ids = list(island_files.keys())

    for i in range(len(island_ids)):
        for j in range(i + 1, len(island_ids)):
            id_a, id_b = island_ids[i], island_ids[j]
            set_a = island_files[id_a]["files"]
            set_b = island_files[id_b]["files"]
            overlap = set_a.intersection(set_b)
            if overlap:
                conflicts.append((id_a, id_b, overlap))

    print("-" * 60)
    if not conflicts:
        print(f"[WAVE-1 SUCCESS] Verificación completada: 10 islas de archivos disjuntas. 0 conflictos.")
        return True
    else:
        print(f"[WAVE-1 ERROR] Se detectaron {len(conflicts)} conflictos entre islas:")
        for id_a, id_b, overlap in conflicts:
            print(f"  Conflict between {id_a} and {id_b}: {overlap}")
        return False


def main():
    repo_root = Path(__file__).resolve().parent.parent
    wave_target = sys.argv[1] if len(sys.argv) > 1 else "wave-1"

    if wave_target in ["wave-1", "wave1", "all"]:
        success = verify_wave_1(repo_root)
        if not success:
            sys.exit(1)
    else:
        print(f"Wave desconocida: {wave_target}. Uso: python3 scripts/harness-file-islands.py wave-1")
        sys.exit(1)


if __name__ == "__main__":
    main()
