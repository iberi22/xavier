//! CLI handlers for `xavier memory encrypt-private` / `decrypt-private`.
//!
//! Thin shell over `xavier::memory::sqlite_vec_store::at_rest` (job engine,
//! cursor tables, verified backup). Dry run is the default and never writes
//! or creates a key.

use anyhow::Result;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use xavier::memory::sqlite_vec_store::at_rest::{self, DecryptSource, JobOptions, JobReport};

/// Resolve the local vec-store DB path (same order as the store config).
fn vec_db_path() -> PathBuf {
    xavier::memory::sqlite_vec_store::VecSqliteStoreConfig::from_env().path
}

/// Arguments of `encrypt-private`.
#[derive(Debug, Clone)]
pub struct EncryptPrivateArgs {
    pub dry_run: bool,
    pub apply: bool,
    pub backup_path: Option<PathBuf>,
    pub batch: usize,
    pub max_rows: Option<u64>,
    pub rate_limit_ms: u64,
    pub resume: bool,
    pub online: bool,
}

/// Arguments of `decrypt-private`.
#[derive(Debug, Clone)]
pub struct DecryptPrivateArgs {
    pub dry_run: bool,
    pub apply: bool,
    pub backup_path: Option<PathBuf>,
    pub from_backup: Option<PathBuf>,
    pub recovery_code: Option<String>,
    pub batch: usize,
    pub resume: bool,
    pub online: bool,
}

fn print_census(db: &std::path::Path) -> Result<at_rest::DryRunReport> {
    let rep = at_rest::dry_run_report(db)?;
    println!("   DB: {}", db.display());
    println!("   Rows total:               {}", rep.total);
    println!("   Plaintext, would encrypt: {}", rep.plaintext_private);
    println!(
        "   Plaintext, explicit public (kept): {}",
        rep.plaintext_public
    );
    println!("   Already XRK1 (node record key): {}", rep.already_xrk1);
    println!("   Already XDK2 (default space):   {}", rep.already_xdk2);
    if rep.legacy_other > 0 {
        println!("   Legacy KEK rows (not touched): {}", rep.legacy_other);
    }
    println!("   Plaintext rows by declared clearance:");
    for (k, v) in &rep.plaintext_by_clearance {
        println!("     {k}: {v}");
    }
    println!(
        "   Default-space keystore: {}",
        if rep.keystore_exists {
            "present"
        } else {
            "absent (would be created on --apply)"
        }
    );
    if rep.already_xrk1 > 0 {
        println!("   Note: XRK1 rows are classified only when decrypted during --apply.");
    }
    Ok(rep)
}

fn stop_flag() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let f = flag.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("SIGINT: finishing the current batch, then stopping (resume with --resume)");
            f.store(true, Ordering::SeqCst);
        }
    });
    flag
}

fn print_report(rep: &JobReport, db: &std::path::Path) {
    println!(
        "   job {}: scanned {}, changed {}, public/kept {}, already {}, failed {}",
        rep.job_id, rep.scanned, rep.changed, rep.skipped_public, rep.skipped_done, rep.failed
    );
    if let Some(p) = &rep.backup_path {
        println!(
            "   Backup: {} (sha256 {})",
            p.display(),
            rep.backup_sha256.as_deref().unwrap_or("?")
        );
    }
    if rep.failed > 0 {
        println!("   Failures (see table memory_encrypt_failures):");
        if let Ok(f) = at_rest::job_failures(db, rep.job_id) {
            for (id, reason) in f.iter().take(20) {
                println!("     {id}: {reason}");
            }
        }
    }
    if !rep.complete {
        println!("   Stopped before completion: run again with --resume.");
    }
}

/// `xavier memory encrypt-private`.
pub async fn handle_encrypt_private(args: EncryptPrivateArgs) -> Result<()> {
    let do_apply = args.apply && !args.dry_run;
    let db = vec_db_path();
    println!("Xavier encrypt-private (default workspace)");
    if !db.exists() {
        anyhow::bail!("database not found at {}", db.display());
    }
    if !do_apply {
        print_census(&db)?;
        println!("DRY-RUN. Nothing was written and no key was created.");
        println!("To apply: xavier memory encrypt-private --apply --backup-path <file-or-dir>");
        return Ok(());
    }
    let mut opts = JobOptions::new(db.clone());
    opts.backup_path = args.backup_path;
    opts.batch = args.batch;
    opts.max_rows = args.max_rows;
    opts.rate_limit_ms = args.rate_limit_ms;
    opts.resume = args.resume;
    opts.online = args.online;
    opts.stop = Some(stop_flag());
    let rep = tokio::task::spawn_blocking(move || {
        at_rest::apply_encrypt(&opts, &mut |code| {
            println!();
            println!("A new default-space keystore was created.");
            println!("RECOVERY CODE (shown ONCE, store it offline, it cannot be shown again):");
            println!("    {code}");
            println!();
        })
    })
    .await??;
    print_report(&rep, &db);
    if rep.complete {
        println!(
            "Verified {} migrated row(s); FTS rebuilt, WAL truncated, VACUUM done.",
            rep.verified
        );
    }
    Ok(())
}

/// `xavier memory decrypt-private`.
pub async fn handle_decrypt_private(args: DecryptPrivateArgs) -> Result<()> {
    let do_apply = args.apply && !args.dry_run;
    let db = vec_db_path();
    println!("Xavier decrypt-private (default workspace)");
    if !db.exists() {
        anyhow::bail!("database not found at {}", db.display());
    }
    if !do_apply {
        let rep = print_census(&db)?;
        println!(
            "DRY-RUN. {} XDK2 row(s) would be restored to plaintext.",
            rep.already_xdk2
        );
        println!("To apply: xavier memory decrypt-private --apply --backup-path <file-or-dir> [--from-backup <file>]");
        return Ok(());
    }
    let mut opts = JobOptions::new(db.clone());
    opts.backup_path = args.backup_path;
    opts.batch = args.batch;
    opts.resume = args.resume;
    opts.online = args.online;
    opts.stop = Some(stop_flag());
    let source = match (args.from_backup, args.recovery_code) {
        (Some(p), _) => DecryptSource::FromBackup(p),
        (None, Some(code)) => {
            DecryptSource::Key(Some(at_rest::unlock_default_with_recovery(&code)?))
        }
        (None, None) => DecryptSource::Key(None),
    };
    let rep = tokio::task::spawn_blocking(move || at_rest::apply_decrypt(&opts, source)).await??;
    print_report(&rep, &db);
    Ok(())
}
