#!/usr/bin/env python3
"""
Test suite for the FTS5 Schema Migration Script (scripts/migrations/fts5-v2.py).
"""

import os
import shutil
import sqlite3
import subprocess
import unittest

SCRIPT_PATH = "scripts/migrations/fts5-v2.py"


class TestFTS5Migration(unittest.TestCase):
    def setUp(self):
        self.test_dir = "temp_test_migration"
        os.makedirs(self.test_dir, exist_ok=True)
        self.db_path = os.path.join(self.test_dir, "test_store.sqlite3")

    def tearDown(self):
        if os.path.isdir(self.test_dir):
            shutil.rmtree(self.test_dir)

    def create_legacy_database(self, include_records=True):
        """Create a database with the old memory_fts schema."""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()

        # Create memory_records table if requested
        if include_records:
            cursor.execute(
                """
                CREATE TABLE memory_records (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT,
                    path TEXT,
                    content TEXT,
                    metadata TEXT,
                    created_at TEXT,
                    updated_at TEXT
                )
                """
            )
            cursor.execute(
                "INSERT INTO memory_records (id, workspace_id, path, content, metadata) VALUES (?, ?, ?, ?, ?)",
                (
                    "rec1",
                    "default",
                    "doc1.md",
                    "this is document one content",
                    '{"title": "Document One Metadata Title", "category": "notes"}',
                ),
            )
            cursor.execute(
                "INSERT INTO memory_records (id, workspace_id, path, content, metadata) VALUES (?, ?, ?, ?, ?)",
                (
                    "rec2",
                    "default",
                    "doc2.md",
                    "this is document two content",
                    '{"category": "no-title-metadata"}',  # No title key inside JSON
                ),
            )

        # Create old memory_fts schema table
        cursor.execute(
            """
            CREATE VIRTUAL TABLE memory_fts USING fts5(
                id UNINDEXED,
                path,
                content,
                code_tokens
            )
            """
        )

        # Populate legacy FTS records
        cursor.execute(
            "INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?, ?, ?, ?)",
            ("rec1", "doc1.md", "this is document one content", "doc1 md document one"),
        )
        cursor.execute(
            "INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?, ?, ?, ?)",
            ("rec2", "doc2.md", "this is document two content", "doc2 md document two"),
        )

        conn.commit()
        conn.close()

    def test_help_menu(self):
        """Test that running --help shows usage options and exits successfully."""
        result = subprocess.run(
            ["python3", SCRIPT_PATH, "--help"],
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertIn("FTS5 Schema Migration Script", result.stdout)
        self.assertIn("--dry-run", result.stdout)
        self.assertIn("--apply", result.stdout)
        self.assertIn("--db-path", result.stdout)

    def test_dry_run_mismatch(self):
        """Test dry-run option detects mismatch without applying changes."""
        self.create_legacy_database()

        # Run dry run
        result = subprocess.run(
            ["python3", SCRIPT_PATH, "--dry-run", "--db-path", self.db_path],
            capture_output=True,
            text=True,
            check=True,
        )

        self.assertIn("Schema mismatch detected. Migration is required", result.stdout)
        self.assertIn("Mode: DRY-RUN", result.stdout)
        self.assertIn("WOULD be migrated", result.stdout)

        # Verify database is unchanged
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute("PRAGMA table_info(memory_fts)")
        cols = [r[1] for r in cursor.fetchall()]
        conn.close()

        # Should only have old columns
        self.assertEqual(cols, ["id", "path", "content", "code_tokens"])

        # Confirm no backup file was created
        backup_file = self.db_path + ".backup"
        self.assertFalse(os.path.exists(backup_file))

    def test_apply_mismatch_with_records(self):
        """Test apply option performs migration and extracts metadata from memory_records."""
        self.create_legacy_database(include_records=True)

        # Run apply
        result = subprocess.run(
            ["python3", SCRIPT_PATH, "--apply", "--db-path", self.db_path],
            capture_output=True,
            text=True,
            check=True,
        )

        self.assertIn("Migration verification PASSED", result.stdout)
        self.assertIn("Successfully created backup", result.stdout)

        # Confirm backup file exists
        backup_file = self.db_path + ".backup"
        self.assertTrue(os.path.exists(backup_file))

        # Connect to migrated DB and verify structure and data
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()

        # Verify columns
        cursor.execute("PRAGMA table_info(memory_fts)")
        cols = [r[1] for r in cursor.fetchall()]
        self.assertEqual(cols, ["id", "title", "path", "content", "code_tokens", "metadata"])

        # Verify migrated rows
        cursor.execute("SELECT id, title, path, content, code_tokens, metadata FROM memory_fts ORDER BY id")
        rows = cursor.fetchall()
        self.assertEqual(len(rows), 2)

        # Row 1 check (with title key in json metadata)
        rec1 = rows[0]
        self.assertEqual(rec1[0], "rec1")
        self.assertEqual(rec1[1], "Document One Metadata Title")  # Extracted title
        self.assertEqual(rec1[2], "doc1.md")
        self.assertEqual(rec1[3], "this is document one content")
        self.assertEqual(rec1[4], "doc1 md document one")
        self.assertEqual(rec1[5], '{"title": "Document One Metadata Title", "category": "notes"}')  # Full metadata

        # Row 2 check (no title key in json metadata)
        rec2 = rows[1]
        self.assertEqual(rec2[0], "rec2")
        self.assertEqual(rec2[1], "")  # Default title
        self.assertEqual(rec2[2], "doc2.md")
        self.assertEqual(rec2[3], "this is document two content")
        self.assertEqual(rec2[4], "doc2 md document two")
        self.assertEqual(rec2[5], '{"category": "no-title-metadata"}')

        conn.close()

    def test_apply_mismatch_no_records_fallback(self):
        """Test apply option performs migration cleanly even when memory_records doesn't exist."""
        self.create_legacy_database(include_records=False)

        # Run apply
        result = subprocess.run(
            ["python3", SCRIPT_PATH, "--apply", "--db-path", self.db_path],
            capture_output=True,
            text=True,
            check=True,
        )

        self.assertIn("Migration verification PASSED", result.stdout)

        # Connect to migrated DB and verify structure and default fallbacks
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()

        # Verify columns
        cursor.execute("PRAGMA table_info(memory_fts)")
        cols = [r[1] for r in cursor.fetchall()]
        self.assertEqual(cols, ["id", "title", "path", "content", "code_tokens", "metadata"])

        # Verify migrated rows with defaults
        cursor.execute("SELECT id, title, path, content, code_tokens, metadata FROM memory_fts ORDER BY id")
        rows = cursor.fetchall()
        self.assertEqual(len(rows), 2)

        self.assertEqual(rows[0][0], "rec1")
        self.assertEqual(rows[0][1], "")  # Fallback empty title
        self.assertEqual(rows[0][5], "{}")  # Fallback empty metadata json

        conn.close()

    def test_idempotent_behavior(self):
        """Test running migration again on already migrated schema is a clean no-op."""
        self.create_legacy_database(include_records=True)

        # Run apply once
        subprocess.run(
            ["python3", SCRIPT_PATH, "--apply", "--db-path", self.db_path],
            capture_output=True,
            text=True,
            check=True,
        )

        # Run apply second time
        result = subprocess.run(
            ["python3", SCRIPT_PATH, "--apply", "--db-path", self.db_path],
            capture_output=True,
            text=True,
            check=True,
        )

        self.assertIn("Schema matches the expected v2 FTS5 columns. No action required", result.stdout)


if __name__ == "__main__":
    unittest.main()
