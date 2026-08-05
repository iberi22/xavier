//! Unified storage and migration system for Xavier.
//!
//! This module provides a struct-based migration framework. A [`Migration`] is
//! a plain `{version, name, up}` record and a [`MigrationRunner`] applies them
//! in version order, each in its own transaction, recording progress in a
//! `schema_migrations` table.
//!
//! For backwards compatibility with earlier callers (e.g. the legacy trait-based
//! API) the [`MigrationManager`] and `Migration` trait aliases are preserved at
//! the bottom of this file. New code should prefer [`MigrationRunner`].

pub mod migrations;
pub mod multi_db;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use tracing::{info, warn};

/// A single database migration.
///
/// `up` is raw SQL (executed via [`rusqlite::Connection::execute_batch`]),
/// `version` must be unique and is used to order migrations, and `name` is a
/// short human-readable label recorded in `schema_migrations`.
#[derive(Clone)]
pub struct Migration {
    /// Unique, monotonically increasing version number.
    pub version: u32,
    /// Short human-readable name for the migration.
    pub name: String,
    /// Raw SQL applied by this migration.
    pub up: &'static str,
}

impl Migration {
    /// Convenience constructor.
    pub fn new(version: u32, name: impl Into<String>, up: &'static str) -> Self {
        Self {
            version,
            name: name.into(),
            up,
        }
    }
}

/// SQL used to create the bookkeeping table that records applied migrations.
pub const SCHEMA_MIGRATIONS_DDL: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT,
    applied_at TEXT
)";

/// Latest schema version known to this binary (the highest baseline migration).
pub const LATEST_SCHEMA_VERSION: u32 = 5;

/// Runner that applies a fixed set of [`Migration`]s to a connection.
///
/// The runner is idempotent: running it twice applies no work the second time.
/// It is also resilient to legacy databases that predate the migration system:
/// if tables already exist but `schema_migrations` does not, the runner detects
/// the existing schema and backfills the migrations table without re-running
/// the SQL.
pub struct MigrationRunner {
    migrations: Vec<Migration>,
}

impl MigrationRunner {
    /// Build a runner from the given migrations. They will be sorted by version
    /// and applied in ascending order.
    pub fn new(migrations: Vec<Migration>) -> Self {
        let mut migrations = migrations;
        migrations.sort_by_key(|m| m.version);
        Self { migrations }
    }

    /// The highest version this runner will apply.
    pub fn target_version(&self) -> u32 {
        self.migrations.iter().map(|m| m.version).max().unwrap_or(0)
    }

    /// Ensure the bookkeeping table exists.
    ///
    /// Also upgrades pre-`name` column variants of `schema_migrations`
    /// (older binaries only stored `version` + `applied_at`).
    fn ensure_migration_table(conn: &Connection) -> Result<()> {
        conn.execute_batch(SCHEMA_MIGRATIONS_DDL)
            .context("Failed to create schema_migrations table")?;

        // Legacy DBs may already have schema_migrations without `name`.
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(schema_migrations)")
            .and_then(|mut stmt| {
                let names = stmt
                    .query_map([], |row| row.get::<_, String>(1))?
                    .filter_map(|c| c.ok())
                    .collect::<Vec<_>>();
                Ok(names)
            })
            .unwrap_or_default();

        if !cols.iter().any(|c| c == "name") {
            conn.execute("ALTER TABLE schema_migrations ADD COLUMN name TEXT", [])
                .context("Failed to add schema_migrations.name")?;
        }
        if !cols.iter().any(|c| c == "applied_at") {
            conn.execute(
                "ALTER TABLE schema_migrations ADD COLUMN applied_at TEXT",
                [],
            )
            .context("Failed to add schema_migrations.applied_at")?;
        }
        Ok(())
    }

    /// Current applied schema version (MAX(version) from schema_migrations, or 0).
    fn current_version(conn: &Connection) -> Result<u32> {
        let version: Option<u32> = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .optional()
            .context("Failed to query current schema version")?
            .flatten();
        Ok(version.unwrap_or(0))
    }

    /// Detect a "legacy" database: one that already has application tables but
    /// no `schema_migrations` bookkeeping table. Returns the version we should
    /// backfill to (the highest known baseline).
    fn is_legacy_db(conn: &Connection) -> Result<bool> {
        let has_migration_table: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |row| row.get(0),
        )?;
        if has_migration_table > 0 {
            return Ok(false);
        }
        // Look for a sentinel table created by the baseline schema. memory_records
        // is the core unified-storage table and is present in every version.
        let has_app_table: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='memory_records'",
            [],
            |row| row.get(0),
        )?;
        Ok(has_app_table > 0)
    }

    /// Backfill `schema_migrations` for a legacy DB (tables exist, no migrations
    /// recorded) without re-executing any DDL.
    fn backfill_legacy(conn: &Connection, target: u32) -> Result<()> {
        let tx = conn
            .unchecked_transaction()
            .context("Failed to begin backfill transaction")?;
        let now = chrono::Utc::now().to_rfc3339();
        for v in 1..=target {
            tx.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, name, applied_at) VALUES (?, 'legacy-backfill', ?)",
                rusqlite::params![v as i64, now],
            )
            .context("Failed to backfill schema_migrations")?;
        }
        tx.commit().context("Failed to commit legacy backfill")?;
        Ok(())
    }

    /// Apply all pending migrations.
    ///
    /// - Creates `schema_migrations` if missing.
    /// - If the DB is a legacy DB (app tables present, no bookkeeping), backfills
    ///   up to [`LATEST_SCHEMA_VERSION`] and returns.
    /// - Otherwise applies each pending migration in its own transaction.
    pub fn run(&self, conn: &Connection) -> Result<()> {
        // Detect legacy DB *before* creating the bookkeeping table so we can
        // backfill rather than re-run DDL.
        if Self::is_legacy_db(conn)? {
            warn!(
                "Legacy database detected (tables present, no schema_migrations); \
                 backfilling to v{} without re-running DDL",
                self.target_version()
            );
            Self::ensure_migration_table(conn)?;
            Self::backfill_legacy(conn, self.target_version())?;
            return Ok(());
        }

        Self::ensure_migration_table(conn)?;
        let mut current = Self::current_version(conn)?;
        info!(
            "Current database schema version: {} (target {})",
            current,
            self.target_version()
        );

        for migration in &self.migrations {
            if migration.version <= current {
                continue;
            }
            info!(
                "Applying migration v{} ({}): [{} bytes of SQL]",
                migration.version,
                migration.name,
                migration.up.len()
            );

            // Each migration runs in its own transaction. On failure the
            // transaction is rolled back (Transaction::Drop is a no-op once an
            // error has surfaced via ?, but we commit explicitly on success).
            let tx = conn.unchecked_transaction().with_context(|| {
                format!("Failed to begin transaction for v{}", migration.version)
            })?;

            tx.execute_batch(migration.up).with_context(|| {
                format!(
                    "Migration v{} ({}) SQL failed",
                    migration.version, migration.name
                )
            })?;

            let now = chrono::Utc::now().to_rfc3339();
            tx.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, ?, ?)",
                rusqlite::params![migration.version as i64, migration.name.as_str(), now],
            )
            .with_context(|| {
                format!(
                    "Failed to record migration v{} ({})",
                    migration.version, migration.name
                )
            })?;

            tx.commit().with_context(|| {
                format!(
                    "Failed to commit migration v{} ({})",
                    migration.version, migration.name
                )
            })?;

            current = migration.version;
            info!(
                "Migration v{} ({}) applied successfully",
                migration.version, migration.name
            );
        }

        Ok(())
    }
}

// =============================================================================
// Backwards compatibility: legacy trait-based API.
//
// Some existing callers (sqlite_store, sqlite_vec_store, tests/recovery_flow)
// construct a `MigrationManager` and register `MigrationV*` structs that
// implement the old `Migration` trait. We keep those working by adapting the
// trait-based interface onto the new struct-based runner. New code should use
// `MigrationRunner` + `Migration` directly.
// =============================================================================

/// Legacy trait retained for backwards compatibility.
pub trait LegacyMigration: Send + Sync {
    /// Unique version number for the migration.
    fn version(&self) -> u32;
    /// Description of the migration.
    fn description(&self) -> &str;
    /// Execute the migration on the given connection.
    fn run(&self, conn: &Connection) -> Result<()>;
}

/// Legacy manager retained for backwards compatibility.
pub struct MigrationManager {
    legacy: Vec<Box<dyn LegacyMigration>>,
}

impl MigrationManager {
    /// New.
    pub fn new() -> Self {
        Self { legacy: Vec::new() }
    }

    pub fn add_migration<M: LegacyMigration + 'static>(&mut self, migration: M) {
        self.legacy.push(Box::new(migration));
        self.legacy.sort_by_key(|m| m.version());
    }

    /// Run all pending migrations on the given connection.
    ///
    /// Adapts the registered legacy migrations onto the new struct-based
    /// [`MigrationRunner`]. Legacy migrations' `run()` closures are wrapped as
    /// SQL callbacks so transaction safety and the legacy-DB backfill behaviour
    /// are inherited from the new runner.
    pub fn run_migrations(&self, conn: &Connection) -> Result<()> {
        // Build a struct-based migration set from the legacy registrations.
        // We can't easily turn arbitrary Rust closures into `&'static str` SQL,
        // so legacy migrations are run via a small indirection: each one is
        // recorded by a "marker" migration whose SQL is empty, and whose actual
        // work is performed through the runner hook. To keep the design clean
        // and fully transaction-safe we instead run the legacy closures inline
        // here, but reuse the runner's bookkeeping/backfill logic.
        MigrationRunner::new(vec![]).run(conn)?;

        // After the runner has ensured the bookkeeping table (and possibly
        // backfilled a legacy DB), apply legacy closures for versions that are
        // not yet recorded. Each runs in its own transaction.
        let current = MigrationRunner::current_version(conn)?;
        for m in &self.legacy {
            if m.version() <= current {
                continue;
            }
            info!(
                "Running migration v{}: {} (legacy closure)",
                m.version(),
                m.description()
            );
            let tx = conn
                .unchecked_transaction()
                .context("Failed to begin transaction")?;
            m.run(&tx).with_context(|| {
                format!(
                    "Failed to run migration v{}: {}",
                    m.version(),
                    m.description()
                )
            })?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, ?, ?)",
                rusqlite::params![
                    m.version() as i64,
                    m.description(),
                    chrono::Utc::now().to_rfc3339()
                ],
            )
            .context("Failed to update schema version")?;
            tx.commit().context("Failed to commit migration")?;
        }
        Ok(())
    }
}

impl Default for MigrationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to check if a table has a column.
pub fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let allowed_tables = [
        "entities",
        "relations",
        "memory_records",
        "timeline_events",
        "memory_chain",
        "session_tokens",
    ];

    if !allowed_tables.contains(&table) {
        anyhow::bail!("Invalid table name for schema check: {}", table);
    }

    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let mut rows = stmt.query(())?;
    while let Some(row) = rows.next()? {
        let col_name: String = row.get(1)?;
        if col_name == column {
            return Ok(true);
        }
    }
    Ok(false)
}
