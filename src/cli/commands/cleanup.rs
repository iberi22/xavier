//! CLI Cleanup command handler.
//!
//! Purges empty conversation databases older than N days,
//! reports the legacy sqlite store and removes it with the `--apply` flag.

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// Handle the cleanup command.
pub async fn handle_cleanup(dry_run: bool, apply: bool, days: u64) -> Result<()> {
    // Determine dry run mode: default to dry run if apply is not explicitly true
    let is_dry_run = dry_run || !apply;

    println!("╔══════════════════════════════════════════════╗");
    println!("║             Xavier Cleanup Utility           ║");
    println!("╚══════════════════════════════════════════════╝");
    if is_dry_run {
        println!("⚠️  RUNNING IN DRY-RUN MODE. No files will be deleted.");
        println!("   To apply changes, run: xavier cleanup --apply\n");
    } else {
        println!("🚀 APPLYING CHANGES. Deleting empty/expired files...\n");
    }

    // 1. Process Conversations Databases
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let conversations_dir = home.join(".xavier").join("conversations");

    let mut qualified_for_deletion = Vec::new();
    let mut surviving_dbs = Vec::new();

    if conversations_dir.exists() && conversations_dir.is_dir() {
        let entries = std::fs::read_dir(&conversations_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("db") {
                let _filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let metadata = std::fs::metadata(&path)?;
                let file_size = metadata.len();
                let modified: DateTime<Utc> = metadata.modified()?.into();
                let age = Utc::now().signed_duration_since(modified);

                let is_empty = file_size <= 4096;

                // Check age condition
                let matches_age = age.num_days() >= days as i64;

                if is_empty && matches_age {
                    qualified_for_deletion.push((path, file_size, modified));
                } else {
                    surviving_dbs.push((path, file_size, modified));
                }
            }
        }
    }

    println!("💬 CONVERSATIONS DATABASES (in {}):", conversations_dir.display());
    println!("   Total qualified for deletion (empty, <= 4KB): {}", qualified_for_deletion.len());
    println!("   Total active databases surviving: {}", surviving_dbs.len());

    if !qualified_for_deletion.is_empty() {
        println!("\n🗑️  Empty databases to purge:");
        for (path, size, modified) in &qualified_for_deletion {
            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            println!("   - {} ({} bytes, modified: {})", filename, size, modified.to_rfc3339());
        }

        if !is_dry_run {
            let mut deleted_count = 0;
            for (path, _, _) in &qualified_for_deletion {
                if let Err(e) = std::fs::remove_file(path) {
                    eprintln!("   ⚠️  Failed to delete {}: {}", path.display(), e);
                } else {
                    deleted_count += 1;
                    // Remove associated sidecars
                    let wal = path.with_extension("db-wal");
                    let shm = path.with_extension("db-shm");
                    let journal = path.with_extension("db-journal");
                    if wal.exists() { let _ = std::fs::remove_file(wal); }
                    if shm.exists() { let _ = std::fs::remove_file(shm); }
                    if journal.exists() { let _ = std::fs::remove_file(journal); }
                }
            }
            println!("   ✅ Successfully deleted {} empty databases.", deleted_count);
        }
    } else {
        println!("   ✨ No empty databases qualified for deletion.");
    }

    if !surviving_dbs.is_empty() {
        println!("\n🛡️  Active databases surviving:");
        for (path, size, modified) in &surviving_dbs {
            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            println!("   - {} ({} bytes, modified: {})", filename, size, modified.to_rfc3339());
        }

        // Run checkpoints and vacuums on surviving databases to optimize them as per WAL mode technical research
        if !is_dry_run {
            println!("\n⚡ Optimizing surviving active databases (WAL Checkpoint + VACUUM)...");
            for (path, _, _) in &surviving_dbs {
                let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                match rusqlite::Connection::open(path) {
                    Ok(conn) => {
                        // PRAGMA wal_checkpoint(TRUNCATE) to compact WAL
                        let cp_res = conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
                        // VACUUM to reclaim space and defragment
                        let vac_res = conn.execute("VACUUM;", []);

                        match (cp_res, vac_res) {
                            (Ok(_), Ok(_)) => println!("   - Optimized {}", filename),
                            _ => println!("   - Partically optimized / skipped {}", filename),
                        }
                    }
                    Err(_) => {
                        println!("   - Could not open/optimize {}", filename);
                    }
                }
            }
        }
    }

    // 2. Process Legacy SQLite Memory Store
    println!("\n🗄️  LEGACY SQLITE MEMORY STORE:");
    let settings = xavier::settings::XavierSettings::current();

    // Collect possible paths for memory-store.sqlite3 to check
    let mut legacy_paths = Vec::new();
    let default_legacy = xavier::settings::XavierSettings::resolve_data_dir().join("memory-store.sqlite3");
    if default_legacy.exists() {
        legacy_paths.push(default_legacy);
    }

    if !settings.memory.sqlite_path.trim().is_empty() {
        let p = std::path::PathBuf::from(&settings.memory.sqlite_path);
        if p.exists() && !legacy_paths.contains(&p) {
            legacy_paths.push(p);
        }
    }

    if !legacy_paths.is_empty() {
        for legacy_path in legacy_paths {
            let metadata = std::fs::metadata(&legacy_path)?;
            let size = metadata.len();
            let modified: DateTime<Utc> = metadata.modified()?.into();

            println!("   Found legacy store at: {}", legacy_path.display());
            println!("   Size: {} bytes", size);
            println!("   Last modified: {}", modified.to_rfc3339());

            // Report table and row counts
            let mut total_rows = 0;
            let mut table_reports = Vec::new();

            if let Ok(conn) = rusqlite::Connection::open(&legacy_path) {
                let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
                let mut tables = Vec::new();
                if let Ok(mut rows) = stmt.query([]) {
                    while let Ok(Some(row)) = rows.next() {
                        if let Ok(name) = row.get::<_, String>(0) {
                            if !name.starts_with("sqlite_") {
                                tables.push(name);
                            }
                        }
                    }
                }
                drop(stmt);

                for table in tables {
                    let count: i64 = conn
                        .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", table), [], |r| r.get(0))
                        .unwrap_or(0);
                    total_rows += count;
                    table_reports.push(format!("     - {}: {} rows", table, count));
                }
            }

            println!("   Total legacy rows detected: {}", total_rows);
            if !table_reports.is_empty() {
                println!("   Table breakdown:");
                for report in table_reports {
                    println!("{}", report);
                }
            }

            if is_dry_run {
                println!("   👉 Legacy store will be deleted when running with --apply.");
            } else {
                println!("   🗑️  Deleting legacy store...");
                if let Err(e) = std::fs::remove_file(&legacy_path) {
                    eprintln!("   ⚠️  Failed to delete legacy store {}: {}", legacy_path.display(), e);
                } else {
                    println!("   ✅ Successfully deleted legacy store memory-store.sqlite3.");
                    // Remove legacy sidecars
                    let wal = legacy_path.with_extension("sqlite3-wal");
                    let shm = legacy_path.with_extension("sqlite3-shm");
                    let journal = legacy_path.with_extension("sqlite3-journal");
                    if wal.exists() { let _ = std::fs::remove_file(wal); }
                    if shm.exists() { let _ = std::fs::remove_file(shm); }
                    if journal.exists() { let _ = std::fs::remove_file(journal); }
                }
            }
        }
    } else {
        println!("   ✨ No legacy store memory-store.sqlite3 found (already cleaned up or not used).");
    }

    println!("\n✨ Cleanup check finished.");
    Ok(())
}
