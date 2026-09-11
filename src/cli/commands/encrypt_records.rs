//! CLI Encrypt-records command handler.
//!
//! Migrates legacy plaintext rows in the local vec-store database to
//! per-record envelope encryption (`encrypted_dek` / `content_iv` /
//! `metadata_iv`). Reports how many rows were migrated.

use anyhow::{Context, Result};
use std::path::PathBuf;
use xavier::memory::sqlite_vec_store::at_rest;

/// Resolve the local vec-store DB path (same order as the store config).
fn vec_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("XAVIER_MEMORY_VEC_PATH") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    let settings = xavier::settings::XavierSettings::current();
    if !settings.memory.vec_path.trim().is_empty() {
        PathBuf::from(&settings.memory.vec_path)
    } else {
        PathBuf::from(&settings.memory.data_dir)
            .join(xavier::memory::sqlite_vec_store::config::DB_FILENAME)
    }
}

/// Handle the encrypt-records command.
pub async fn handle_encrypt_records(dry_run: bool, apply: bool) -> Result<()> {
    // Same convention as cleanup: default to dry run unless --apply is given.
    let do_apply = apply && !dry_run;
    let db_path = vec_db_path();

    println!("╔══════════════════════════════════════════════╗");
    println!("║        Xavier Encrypt-Records at Rest        ║");
    println!("╚══════════════════════════════════════════════╝");
    println!("   DB: {}", db_path.display());

    if !db_path.exists() {
        anyhow::bail!("database not found at {}", db_path.display());
    }

    match at_rest::resolve_record_key_with_source() {
        (Some(_), at_rest::KeySource::Env) => {
            println!("   Node key: {} (env)", at_rest::RECORD_KEY_ENV);
        }
        (Some(_), at_rest::KeySource::File(p)) => {
            println!("   Node key: existing file {}", p.display());
        }
        (Some(_), at_rest::KeySource::Generated(p)) => {
            println!("   Node key: generated {}", p.display());
        }
        (None, _) => {
            anyhow::bail!(
                "no node record key available (set XAVIER_RECORD_KEY or make the data dir writable)"
            );
        }
        (Some(_), at_rest::KeySource::Unavailable(reason)) => {
            println!("   Node key: available ({reason})");
        }
    }

    let conn = rusqlite::Connection::open(&db_path)
        .with_context(|| format!("cannot open {}", db_path.display()))?;
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_records WHERE encrypted_dek IS NULL OR length(encrypted_dek) = 0",
            [],
            |row| row.get(0),
        )
        .context("migration SELECT failed (is this a Xavier memory DB?)")?;
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_records", [], |row| row.get(0))
        .unwrap_or(0);
    println!("   Rows total: {total}, plaintext pending: {pending}");

    if !do_apply {
        println!("⚠️  DRY-RUN. Nothing was written.");
        println!("   To encrypt {pending} row(s), run: xavier encrypt-records --apply");
        return Ok(());
    }

    let migrated = at_rest::migrate_connection(&conn)?;
    let left: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_records WHERE encrypted_dek IS NULL OR length(encrypted_dek) = 0",
            [],
            |row| row.get(0),
        )
        .unwrap_or(-1);
    println!("✅ Migrated {migrated} row(s). Plaintext rows remaining: {left}.");
    Ok(())
}
