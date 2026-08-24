#!/usr/bin/env python3
"""Normalize stale Windows-local paths in chronicle/devlog assets.

Replaces legacy absolute paths from the old Windows checkout with either
GitHub deep-links (for source references) or the current canonical local
path. Safe to re-run (idempotent): after one pass no target patterns remain.

Scope: docs/devlog/*.md and public/devlog/* (HTML/JSON).
"""
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GH = "https://github.com/iberi22/xavier"
OLD_PREFIX = "file:///e:/scripts-python/xavier/"

# Link-form: file:///e:/scripts-python/xavier/<path>  -> GitHub blob|tree URL
LINK_RE = re.compile(re.escape(OLD_PREFIX) + r'([^\s)"<\\]+)')
# Bare mentions (any slash style): E:\...\scripts-python[\xavier] incl. WSL /mnt/e form
BARE_RE = re.compile(r'(?:[eE]:[\\/]+|/mnt/[eE]/)scripts-python(?:[\\/]xavier)?')


def gh_url(path: str) -> str:
    tail = f"/{path}" if path else ""
    kind = "tree" if path.endswith("/") else "blob"
    return f"{GH}/{kind}/main{tail}"


def normalize(text: str) -> tuple[str, int, int]:
    text, n_links = LINK_RE.subn(lambda m: gh_url(m.group(1)), text)
    text, n_bare = BARE_RE.subn("~/proyectosSWAL/apps/xavier", text)
    return text, n_links, n_bare


def main() -> None:
    targets = sorted(REPO.glob("docs/devlog/*.md")) + sorted(
        p for p in REPO.glob("public/devlog/*")
        if p.suffix in {".html", ".js", ".json", ".css"}
    )
    total_links = total_bare = 0
    for f in targets:
        if f.is_symlink():
            continue
        try:
            raw = f.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if "scripts-python" not in raw:
            continue
        new, n_links, n_bare = normalize(raw)
        f.write_text(new, encoding="utf-8")
        total_links += n_links
        total_bare += n_bare
        print(f"  {f.relative_to(REPO)}: {n_links} links, {n_bare} bare paths")
    print(f"done: {total_links} deep-links, {total_bare} bare paths normalized")
    leftover = sum(
        "scripts-python" in f.read_text(encoding="utf-8", errors="ignore")
        for f in targets
    )
    if leftover:
        print(f"WARNING: {leftover} files still mention scripts-python")
        sys.exit(1)


if __name__ == "__main__":
    main()
