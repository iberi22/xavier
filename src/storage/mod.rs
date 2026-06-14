//! Unified storage and migration system for Xavier.

pub mod migrations;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use tracing::info;

/// A single database migration.
pub trait Migration: Send + Sync {
    /// Unique version number for the migration.
    fn version(&self) -> u32;

    /// Description of the migration.
    fn description(&self) -> &str;

    /// Execute the migration on the given connection.
    fn run(&self, conn: &Connection) -> Result<()>;
}

/// Manager for handling database migrations.
pub struct MigrationManager {
    migrations: Vec<Box<dyn Migration>>,
}

impl MigrationManager {
    /// Create a new migration manager.
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
        }
    }

    /// Add a migration to the manager.
    pub fn add_migration<M: Migration + 'static>(&mut self, migration: M) {
        self.migrations.push(Box::new(migration));
        // Sort migrations by version
        self.migrations.sort_by_key(|m| m.version());
    }

    /// Run all pending migrations on the given connection.
    pub fn run_migrations(&self, conn: &Connection) -> Result<()> {
        self.ensure_migration_table(conn)?;

        let current_version = self.get_current_version(conn)?;
        info!("Current database schema version: {}", current_version);

        for migration in &self.migrations {
            if migration.version() > current_version {
                info!(
                    "Running migration v{}: {}",
                    migration.version(),
                    migration.description()
                );

                migration.run(conn).with_context(|| {
                    format!(
                        "Failed to run migration v{}: {}",
                        migration.version(),
                        migration.description()
                    )
                })?;

                self.update_version(conn, migration.version())?;
            }
        }

        Ok(())
    }

    fn ensure_migration_table(&self, conn: &Connection) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        ).context("Failed to create migration table")?;
        Ok(())
    }

    fn get_current_version(&self, conn: &Connection) -> Result<u32> {
        let version: Option<u32> = conn
            .query_row(
                "SELECT MAX(version) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .optional()
            .context("Failed to query current schema version")?
            .flatten();

        Ok(version.unwrap_or(0))
    }

    fn update_version(&self, conn: &Connection, version: u32) -> Result<()> {
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?)",
            [version],
        ).context("Failed to update schema version")?;
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
