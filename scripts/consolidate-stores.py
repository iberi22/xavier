#!/usr/bin/env python3
"""
Consolidate fragmented vec-store SQLite databases into the canonical store.

Canonical store: apps/xavier/data/vec-store.sqlite3
Legacy stores are detected outside the data/ dir and merged with dedup.

Usage:
    python3 scripts/consolidate-stores.py [--dry-run] [--verbose]
"""

import argparse
import os
import sqlite3
import sys
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class StoreReport:
    path: str
    records: int = 0
    records_merged: int = 0
    duplicates_skipped: int = 0
    has_table: bool = False
    error: str = ""

    @property
    def status(self) -> str:
        if self.error:
            return "ERROR"
        if not self.has_table:
            return "NO_TABLE"
        if self.records == 0:
            return "EMPTY"
        return "HAS_DATA"


REPO_ROOT = Path(__file__).resolve().parent.parent
CANONICAL_STORE = REPO_ROOT / "data" / "vec-store.sqlite3"

# Known legacy locations (relative to repo root or absolute home paths)
LEGACY_PATHS = [
    REPO_ROOT / "vec-store.sqlite3",                         # repo root
    REPO_ROOT / ".xavier" / "vec-store.sqlite3",             # .xavier/
    Path.home() / "proyectosSWAL" / "xavier" / "data" / "vec-store.sqlite3",  # old top-level xavier
    Path.home() / ".local" / "share" / "xavier" / "vec-store.sqlite3",         # XDG local share
]

# Pattern-based discovery: find vec-store*.sqlite3 outside data/
def discover_legacy_stores() -> list[Path]:
    """Find vec-store*.sqlite3 files that are NOT the canonical store."""
    found: list[Path] = []

    # Check explicit paths
    for p in LEGACY_PATHS:
        if p.is_file():
            found.append(p)

    # Pattern search in repo root (excluding data/ and target/)
    for match in REPO_ROOT.glob("vec-store*.sqlite3"):
        if match != CANONICAL_STORE and match.is_file():
            if match not in found:
                found.append(match)

    # Pattern search in ~/.local/share/xavier/
    xdg_dir = Path.home() / ".local" / "share" / "xavier"
    if xdg_dir.is_dir():
        for match in xdg_dir.glob("vec-store*.sqlite3"):
            if match.is_file() and match not in found:
                found.append(match)

    return found


def count_records(conn: sqlite3.Connection, table: str = "memory_records") -> int:
    """Count rows in table, return 0 if table doesn't exist."""
    try:
        cursor = conn.execute(f"SELECT COUNT(*) FROM {table}")
        return cursor.fetchone()[0]
    except sqlite3.OperationalError:
        return 0


def has_table(conn: sqlite3.Connection, table: str = "memory_records") -> bool:
    """Check if table exists."""
    try:
        conn.execute(f"SELECT 1 FROM {table} LIMIT 0")
        return True
    except sqlite3.OperationalError:
        return False


def get_legacy_records(
    conn: sqlite3.Connection,
) -> list[tuple]:
    """Read all records from a legacy store."""
    try:
        cursor = conn.execute(
            "SELECT id, workspace_id, path, content, metadata, embedding "
            "FROM memory_records"
        )
        return cursor.fetchall()
    except sqlite3.OperationalError as e:
        print(f"  WARNING: Could not read records: {e}", file=sys.stderr)
        return []


def insert_or_ignore(
    conn: sqlite3.Connection,
    record: tuple,
    dry_run: bool,
) -> bool:
    """Insert record into canonical store, skip if duplicate.
    Returns True if inserted, False if skipped (duplicate)."""
    _id, workspace_id, path, content, metadata, embedding = record

    # Build dedup key: path + workspace_id, or session_id from metadata
    dedup_key = f"{workspace_id}::{path}"

    if dry_run:
        return True

    try:
        conn.execute(
            "INSERT OR IGNORE INTO memory_records "
            "(id, workspace_id, path, content, metadata, embedding) "
            "VALUES (?, ?, ?, ?, ?, ?)",
            (None, workspace_id, path, content, metadata, embedding),
        )
        return conn.total_changes > 0
    except sqlite3.IntegrityError:
        return False
    except sqlite3.OperationalError as e:
        print(f"  WARNING: Insert failed: {e}", file=sys.stderr)
        return False


def main():
    parser = argparse.ArgumentParser(
        description="Consolidate fragmented vec-store SQLite databases"
    )
    parser.add_argument(
        "--dry-run", action="store_true",
        help="Report only, don't modify the canonical store"
    )
    parser.add_argument(
        "--verbose", "-v", action="store_true",
        help="Show detailed per-record output"
    )
    args = parser.parse_args()

    print("=" * 60)
    print("  Xavier Vec-Store Consolidation Script")
    print("=" * 60)
    print(f"  Canonical store: {CANONICAL_STORE}")
    print(f"  Dry run: {args.dry_run}")
    print()

    # Verify canonical store exists and has the right table
    if not CANONICAL_STORE.is_file():
        print(f"ERROR: Canonical store not found: {CANONICAL_STORE}")
        sys.exit(1)

    canonical_conn = sqlite3.connect(str(CANONICAL_STORE))
    if not has_table(canonical_conn):
        print("ERROR: Canonical store has no memory_records table")
        canonical_conn.close()
        sys.exit(1)

    canonical_before = count_records(canonical_conn)
    print(f"  Canonical store: {canonical_before} records before merge")
    print()

    # Discover legacy stores
    legacy_stores = discover_legacy_stores()
    print(f"  Found {len(legacy_stores)} legacy store(s) to scan:")
    for p in legacy_stores:
        print(f"    - {p}")
    print()

    reports: list[StoreReport] = []
    total_merged = 0
    total_skipped = 0

    for legacy_path in legacy_stores:
        report = StoreReport(path=str(legacy_path))
        print(f"  Scanning: {legacy_path}")

        try:
            # Open read-only via URI
            uri = f"file:{legacy_path}?mode=ro"
            legacy_conn = sqlite3.connect(uri, uri=True)
        except sqlite3.Error as e:
            report.error = str(e)
            reports.append(report)
            print(f"    ERROR: Could not open: {e}")
            continue

        report.has_table = has_table(legacy_conn)
        if not report.has_table:
            reports.append(report)
            print(f"    No memory_records table — skipping")
            legacy_conn.close()
            continue

        report.records = count_records(legacy_conn)
        print(f"    {report.records} records found")

        if report.records == 0:
            reports.append(report)
            legacy_conn.close()
            continue

        # Read and merge records
        records = get_legacy_records(legacy_conn)
        merged = 0
        skipped = 0

        for record in records:
            if args.verbose:
                _id, workspace_id, path, _content, _meta, _emb = record
                print(f"      Record: id={_id}, ws={workspace_id}, path={path}")

            # Insert-or-ignore: INSERT OR IGNORE won't error on duplicates
            if not args.dry_run:
                before = canonical_conn.total_changes
                try:
                    canonical_conn.execute(
                        "INSERT OR IGNORE INTO memory_records "
                        "(workspace_id, path, content, metadata, embedding) "
                        "VALUES (?, ?, ?, ?, ?)",
                        (record[1], record[2], record[3], record[4], record[5]),
                    )
                    after = canonical_conn.total_changes
                    if after > before:
                        merged += 1
                    else:
                        skipped += 1
                except sqlite3.IntegrityError:
                    skipped += 1
                except sqlite3.OperationalError as e:
                    print(f"      WARNING: {e}")
                    skipped += 1
            else:
                merged += 1  # Would merge

        if not args.dry_run:
            canonical_conn.commit()

        report.records_merged = merged
        report.duplicates_skipped = skipped
        total_merged += merged
        total_skipped += skipped

        print(f"    Merged: {merged}, Duplicates skipped: {skipped}")
        reports.append(report)
        legacy_conn.close()

    # Final summary
    canonical_after = count_records(canonical_conn) if not args.dry_run else canonical_before + total_merged
    canonical_conn.close()

    print()
    print("=" * 60)
    print("  CONSOLIDATION SUMMARY")
    print("=" * 60)
    print(f"  {'Store':<60} {'Status':<10} {'Records':<8} {'Merged':<8} {'Dupes':<8}")
    print(f"  {'-'*94}")
    for r in reports:
        print(
            f"  {r.path:<60} {r.status:<10} {r.records:<8} "
            f"{r.records_merged:<8} {r.duplicates_skipped:<8}"
        )
    print(f"  {'-'*94}")
    print(f"  Total merged into canonical:   {total_merged}")
    print(f"  Total duplicates skipped:      {total_skipped}")
    print(f"  Canonical records:             {canonical_before} → {canonical_after}")
    print()

    # Recommendation
    archivable = [r for r in reports if r.status == "EMPTY" or r.status == "NO_TABLE"]
    data_stores = [r for r in reports if r.status == "HAS_DATA"]
    if data_stores:
        print("  ⚠ Legacy stores with data that can be archived:")
        for r in data_stores:
            print(f"    {r.path}")
    if archivable:
        print("  ✅ Empty/no-table stores safe to remove:")
        for r in archivable:
            print(f"    {r.path}")
    if not data_stores and not archivable:
        print("  ✅ No legacy stores found with data.")

    print()
    if args.dry_run:
        print("  (DRY RUN — no changes made)")
    else:
        print("  Consolidation complete.")


if __name__ == "__main__":
    main()
