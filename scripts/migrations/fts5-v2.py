#!/usr/bin/env python3
"""
FTS5 Schema Migration Script for Xavier Unified Storage.

This script detects the current schema of the `memory_fts` virtual table,
compares it with the expected v2 schema (including 'title' and 'metadata' columns),
and performs an idempotent migration if there is a mismatch.

Usage:
    python3 scripts/migrations/fts5-v2.py --dry-run
    python3 scripts/migrations/fts5-v2.py --apply
    python3 scripts/migrations/fts5-v2.py --db-path /path/to/database.sqlite3 --apply
"""

import argparse
import datetime
import json
import os
import shutil
import sqlite3
import sys

# Expected columns for memory_fts v2 virtual table
EXPECTED_COLUMNS = ["id", "title", "path", "content", "code_tokens", "metadata"]


def get_db_candidates():
    """Identify potential SQLite database paths from configuration and data directories."""
    candidates = []

    # Try reading from config/xavier.config.json
    config_path = "config/xavier.config.json"
    if os.path.exists(config_path):
        try:
            with open(config_path, "r", encoding="utf-8") as f:
                config = json.load(f)
                memory_config = config.get("memory", {})

                # Check sqlite_path
                sqlite_path = memory_config.get("sqlite_path")
                if sqlite_path:
                    candidates.append(sqlite_path)

                # Check vec_path
                vec_path = memory_config.get("vec_path")
                if vec_path:
                    candidates.append(vec_path)

                # Check workspaces directory
                workspace_dir = memory_config.get("workspace_dir")
                if workspace_dir and os.path.isdir(workspace_dir):
                    for root, _, files in os.walk(workspace_dir):
                        for file in files:
                            if file.endswith((".sqlite", ".sqlite3", ".db")):
                                candidates.append(os.path.join(root, file))
        except Exception as e:
            print(f"[warn] Failed to parse config file: {e}")

    # Fallback / additional standard locations under current directory
    standard_paths = [
        "data/vec-store.sqlite3",
        "data/memory-store.sqlite3",
    ]
    for p in standard_paths:
        if p not in candidates:
            candidates.append(p)

    # Walk through data directory if it exists to find any leftover sqlite DBs
    if os.path.isdir("data"):
        for root, _, files in os.walk("data"):
            for file in files:
                if file.endswith((".sqlite", ".sqlite3", ".db")):
                    full_path = os.path.join(root, file)
                    if full_path not in candidates:
                        candidates.append(full_path)

    # Filter to only existing database files
    existing_candidates = [os.path.normpath(c) for c in candidates if os.path.isfile(c)]

    # Remove duplicates while preserving order
    seen = set()
    unique_candidates = []
    for c in existing_candidates:
        if c not in seen:
            seen.add(c)
            unique_candidates.append(c)

    return unique_candidates


def get_table_columns(conn, table_name):
    """Retrieve column names of a table."""
    try:
        cursor = conn.cursor()
        cursor.execute(f"PRAGMA table_info({table_name})")
        rows = cursor.fetchall()
        return [row[1] for row in rows]
    except sqlite3.Error:
        return []


def check_table_exists(conn, table_name):
    """Check if a table exists in sqlite_master."""
    cursor = conn.cursor()
    cursor.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name=?", (table_name,)
    )
    return cursor.fetchone() is not None


def create_backup(db_path):
    """Create a backup of the specified database file."""
    if not os.path.exists(db_path):
        return None

    backup_path = db_path + ".backup"
    if os.path.exists(backup_path):
        # Backup already exists, append timestamp
        timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
        backup_path = f"{db_path}.backup.{timestamp}"

    try:
        shutil.copy2(db_path, backup_path)
        print(f"[info] Successfully created backup of {db_path} at {backup_path}")
        return backup_path
    except Exception as e:
        print(f"[error] Failed to create backup of {db_path}: {e}")
        return None


def run_migration(db_path, dry_run=True):
    """Analyze and migrate the given SQLite database if needed."""
    print("-" * 60)
    print(f"Checking database: {db_path}")

    try:
        # Connect to DB. We use isolation_level=None to control transactions manually.
        conn = sqlite3.connect(db_path, timeout=5)
    except sqlite3.Error as e:
        print(f"[error] Failed to connect to database {db_path}: {e}")
        return False

    try:
        if not check_table_exists(conn, "memory_fts"):
            print("[info] Virtual table 'memory_fts' does not exist in this database. No migration needed.")
            conn.close()
            return True

        current_cols = get_table_columns(conn, "memory_fts")
        print(f"[info] Current columns in 'memory_fts': {current_cols}")
        print(f"[info] Expected columns: {EXPECTED_COLUMNS}")

        # Check for column schema mismatch
        mismatch = set(current_cols) != set(EXPECTED_COLUMNS) or current_cols != EXPECTED_COLUMNS

        if not mismatch:
            print("[info] Schema matches the expected v2 FTS5 columns. No action required.")
            conn.close()
            return True

        print("[info] Schema mismatch detected. Migration is required.")

        if dry_run:
            print("[dry-run] Schema WOULD be migrated to include all expected columns.")
            print("[dry-run] A backup file WOULD be created before writing.")
            conn.close()
            return True

        # Perform the actual migration
        conn.close()  # Close active connection to avoid lock during backup

        # Create Backup
        backup_file = create_backup(db_path)
        if not backup_file:
            print("[error] Aborting migration: Backup creation failed.")
            return False

        # Reconnect to apply the changes
        try:
            conn = sqlite3.connect(db_path, timeout=5)
            cursor = conn.cursor()
        except sqlite3.Error as e:
            print(f"[error] Failed to reconnect to database {db_path}: {e}")
            return False

        try:
            print("[info] Starting migration transaction...")
            cursor.execute("BEGIN TRANSACTION")

            # Check if memory_records table exists to enrich with metadata / title
            has_memory_records = check_table_exists(conn, "memory_records")
            has_metadata_col = False
            if has_memory_records:
                record_cols = get_table_columns(conn, "memory_records")
                has_metadata_col = "metadata" in record_cols

            # 1. Create a temporary v2 virtual table
            print("[info] Creating 'memory_fts_v2' virtual table...")
            cursor.execute(
                """
                CREATE VIRTUAL TABLE memory_fts_v2 USING fts5(
                    id UNINDEXED,
                    title,
                    path,
                    content,
                    code_tokens,
                    metadata
                )
                """
            )

            # 2. Migrate existing data from memory_fts, potentially joining memory_records
            print("[info] Migrating existing FTS5 index records...")
            if has_memory_records and has_metadata_col:
                # Query that extracts title and metadata from JSON if available
                cursor.execute(
                    """
                    INSERT INTO memory_fts_v2(id, title, path, content, code_tokens, metadata)
                    SELECT
                        f.id,
                        COALESCE(json_extract(m.metadata, '$.title'), '') AS title,
                        f.path,
                        f.content,
                        f.code_tokens,
                        COALESCE(m.metadata, '{}') AS metadata
                    FROM memory_fts f
                    LEFT JOIN memory_records m ON f.id = m.id
                    """
                )
            else:
                # Fallback mapping if memory_records is unavailable or missing metadata
                cursor.execute(
                    """
                    INSERT INTO memory_fts_v2(id, title, path, content, code_tokens, metadata)
                    SELECT id, '' AS title, path, content, code_tokens, '{}' AS metadata FROM memory_fts
                    """
                )

            # 3. Drop original virtual table
            print("[info] Dropping old 'memory_fts' table...")
            cursor.execute("DROP TABLE memory_fts")

            # 4. Rename memory_fts_v2 to memory_fts
            print("[info] Renaming 'memory_fts_v2' to 'memory_fts'...")
            cursor.execute("ALTER TABLE memory_fts_v2 RENAME TO memory_fts")

            conn.commit()
            print("[info] Transaction committed successfully!")

            # Verify schema post-migration
            new_cols = get_table_columns(conn, "memory_fts")
            if new_cols == EXPECTED_COLUMNS:
                print("[info] Migration verification PASSED. Columns are correctly aligned.")
                return True
            else:
                print(f"[error] Migration verification FAILED. Actual columns: {new_cols}")
                return False

        except sqlite3.OperationalError as oe:
            # Check if locked
            if "locked" in str(oe).lower() or "busy" in str(oe).lower():
                print("\n" + "=" * 60)
                print("[error] Database is LOCKED by running Xavier!")
                print("Please stop Xavier server first before executing migrations.")
                print("=" * 60 + "\n")
            else:
                print(f"[error] Operational SQL error: {oe}")
            try:
                conn.rollback()
            except sqlite3.Error:
                pass
            return False
        except Exception as ex:
            print(f"[error] Error occurred during migration execution: {ex}")
            try:
                conn.rollback()
            except sqlite3.Error:
                pass
            return False
        finally:
            conn.close()

    except Exception as e:
        print(f"[error] Unexpected error in run_migration: {e}")
        try:
            conn.close()
        except Exception:
            pass
        return False


def main():
    parser = argparse.ArgumentParser(
        description="FTS5 Schema Migration Script to upgrade memory_fts schema safely and idempotently."
    )
    group = parser.add_mutually_exclusive_group(required=False)
    group.add_argument(
        "--dry-run",
        action="store_true",
        help="Perform a dry-run check without applying any modifications (Default behavior).",
    )
    group.add_argument(
        "--apply",
        action="store_true",
        help="Apply migrations to matching databases.",
    )
    parser.add_argument(
        "--db-path",
        default=None,
        help="Path to a specific SQLite database file to check/migrate. If not provided, it scans standard directories.",
    )

    args = parser.parse_args()

    # Default to dry-run if neither is explicitly passed
    is_dry_run = True
    if args.apply:
        is_dry_run = False

    print("=" * 60)
    print("Xavier FTS5 Schema Migration Utility")
    print("=" * 60)
    if is_dry_run:
        print("Mode: DRY-RUN (No changes will be written)")
    else:
        print("Mode: APPLY (Database changes will be executed after creating a backup)")

    if args.db_path:
        # Specific database path target
        if not os.path.isfile(args.db_path):
            print(f"[error] Provided database path is not a file: {args.db_path}")
            return 1
        databases = [args.db_path]
    else:
        # Autodetect databases
        print("[info] Auto-detecting SQLite databases...")
        databases = get_db_candidates()
        if not databases:
            print("[warn] No candidate SQLite databases found for schema checks.")
            return 0
        print(f"[info] Found {len(databases)} candidate database(s): {databases}")

    success = True
    for db in databases:
        if not run_migration(db, dry_run=is_dry_run):
            success = False

    print("=" * 60)
    if success:
        print("FTS5 Migration script execution completed successfully.")
        return 0
    else:
        print("FTS5 Migration script encountered errors on one or more databases.")
        return 1


if __name__ == "__main__":
    sys.exit(main())
