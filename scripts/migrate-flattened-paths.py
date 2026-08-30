#!/usr/bin/env python3
"""Migración one-shot: reconstruye la jerarquía de paths aplanados en vec-store.

El bug (fixed en cli/handlers/memory.rs) guardaba los paths SIN barras:
  "hermes2026-08-1720260817_00355" en vez de "hermes/2026-08-17/20260817_00355".

Patrón reconocible: <prefijo><YYYY-MM-DD><resto> — se insertan las barras usando
el ancla de fecha. NO se puede recuperar: acentos perdidos por retain(ascii),
ni separadores originales (se unieron con separador vacío). La migración solo
restaura la forma canónica para los paths con ancla de fecha.

Uso:
  python3 scripts/migrate-flattened-paths.py [--db path] [--dry-run]

Hace un backup .bak del vec-store antes de tocar nada.
"""
import argparse
import re
import shutil
import sqlite3
import sys

DATE_ANCHOR = re.compile(r"^([a-zA-Z0-9_.\-]+?)(\d{4}-\d{2}-\d{2})(.*)$")

# Prefijos que usan la convención hermes/YYYY-MM-DD/... (no tocar otros)
KNOWN_PREFIXES = ("hermes", "antigravity", "session", "workspaces", "memory")


def migrate(db_path: str, dry_run: bool) -> int:
    shutil.copy2(db_path, db_path + ".bak")
    conn = sqlite3.connect(db_path)
    cur = conn.cursor()
    rows = cur.execute(
        "SELECT rowid, path FROM memory_records WHERE path NOT LIKE '%/%'"
    ).fetchall()

    changed = 0
    for rowid, path in rows:
        m = DATE_ANCHOR.match(path)
        if not m:
            continue
        prefix, date, rest = m.group(1), m.group(2), m.group(3)
        if prefix not in KNOWN_PREFIXES:
            continue
        new_path = f"{prefix}/{date}/{rest}"
        if dry_run:
            print(f"  {path}  ->  {new_path}")
        else:
            cur.execute(
                "UPDATE memory_records SET path = ?, updated_at = updated_at WHERE rowid = ?",
                (new_path, rowid),
            )
        changed += 1

    if not dry_run:
        conn.commit()
    conn.close()
    return changed


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="data/vec-store.sqlite3")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    n = migrate(args.db, args.dry_run)
    mode = "DRY-RUN (sin cambios)" if args.dry_run else "MIGRADO"
    print(f"{mode}: {n} paths reconstruidos en {args.db} (backup: {args.db}.bak)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
