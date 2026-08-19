//! Binary for resumable backfill of memory embeddings using Ollama (nomic-embed-text, 768-dim)

use anyhow::{Context, Result};
use clap::Parser;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use xavier::embedding::ollama::OllamaEmbedder;
use xavier::embedding::Embedder;
use xavier::memory::sqlite_vec_store::vector;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Resumable backfill for memory embeddings with Ollama",
    long_about = None
)]
struct Args {
    /// Path to Xavier SQLite DB file
    #[arg(long, default_value = "vec-store.sqlite3")]
    db_path: PathBuf,

    /// Batch size for embedding processing
    #[arg(long, default_value_t = 64)]
    batch_size: usize,

    /// Dry run mode (do not update database)
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Resume from checkpoint (default: true)
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    resume: bool,
}

struct RecordToProcess {
    rowid: i64,
    id: String,
    workspace_id: String,
    content: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    info!(
        "Starting backfill binary: db_path={:?}, batch_size={}, dry_run={}, resume={}",
        args.db_path, args.batch_size, args.dry_run, args.resume
    );

    // Register sqlite-vec extension
    let _ = vector::register_sqlite_vec_extension();

    // Setup signal flag for graceful shutdown
    let shutdown = Arc::new(AtomicBool::new(false));
    let s_clone = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        info!("Termination signal received. Requesting graceful shutdown...");
        s_clone.store(true, Ordering::SeqCst);
    });

    // 1. Resolve DB path and open connection
    let db_path = if !args.db_path.exists() {
        let candidates = [
            args.db_path.clone(),
            PathBuf::from("data/vec-store.sqlite3"),
            dirs::home_dir()
                .map(|h| h.join(".xavier/vec-store.sqlite3"))
                .unwrap_or_default(),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or(args.db_path)
    } else {
        args.db_path
    };

    info!("Connecting to database at {:?}", db_path);
    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open database at {:?}", db_path))?;

    // Create memory_embeddings_768 table and backfill_checkpoint table if missing
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_embeddings_768 (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            embedding BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS backfill_checkpoint (
            key TEXT PRIMARY KEY,
            value TEXT,
            updated_at DATETIME
        );",
    )?;

    // 2. Initialize Ollama embedder
    let embedder = OllamaEmbedder::from_env()?;
    info!("Ollama embedder initialized for model {}", embedder.model());

    // 3. Determine starting rowid from checkpoint
    let mut start_rowid: i64 = 0;
    if args.resume {
        let checkpoint_val: Option<String> = conn
            .query_row(
                "SELECT value FROM backfill_checkpoint WHERE key = 'last_processed_rowid'",
                [],
                |row| row.get(0),
            )
            .ok();

        if let Some(val) = checkpoint_val {
            if let Ok(id) = val.parse::<i64>() {
                start_rowid = id;
                info!("Resuming backfill from rowid > {}", start_rowid);
            }
        }
    }

    // 4. Get total remaining records count
    let total_remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_records WHERE (embedding IS NULL OR length(embedding) < 100) AND rowid > ?",
            params![start_rowid],
            |row| row.get(0),
        )
        .unwrap_or(0);

    info!(
        "Found {} records needing embedding backfill",
        total_remaining
    );
    if total_remaining == 0 {
        info!("No records need backfilling. Done!");
        return Ok(());
    }

    let mut last_processed_rowid = start_rowid;
    let mut processed_count = 0usize;
    let start_time = Instant::now();

    loop {
        if shutdown.load(Ordering::SeqCst) {
            info!("Shutting down gracefully at rowid {}", last_processed_rowid);
            break;
        }

        // Fetch next batch
        let records = {
            let mut stmt = conn.prepare(
                "SELECT rowid, id, workspace_id, content FROM memory_records WHERE (embedding IS NULL OR length(embedding) < 100) AND rowid > ? ORDER BY rowid ASC LIMIT ?"
            )?;

            let rows = stmt
                .query_map(
                    params![last_processed_rowid, args.batch_size as i64],
                    |row| {
                        Ok(RecordToProcess {
                            rowid: row.get(0)?,
                            id: row.get(1)?,
                            workspace_id: row.get(2)?,
                            content: row.get(3)?,
                        })
                    },
                )?
                .filter_map(|r| r.ok())
                .collect::<Vec<_>>();
            rows
        };

        if records.is_empty() {
            info!("Finished processing all matching records.");
            break;
        }

        let batch_len = records.len();
        let batch_last_rowid = records.last().unwrap().rowid;

        // Process batch with embedder
        let mut updates = Vec::with_capacity(batch_len);
        for rec in &records {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            match embedder.encode(&rec.content).await {
                Ok(vector) => {
                    if vector.len() == 768 {
                        let blob = vector::serialize_embedding(&vector);
                        updates.push((rec.id.clone(), rec.workspace_id.clone(), vector, blob));
                    } else {
                        warn!(
                            "Embedder returned vector dimension {} (expected 768) for record {}",
                            vector.len(),
                            rec.id
                        );
                    }
                }
                Err(err) => {
                    warn!("Failed to encode record {}: {}", rec.id, err);
                }
            }
        }

        if !args.dry_run && !updates.is_empty() {
            let tx = conn.unchecked_transaction()?;
            for (id, workspace_id, vector, blob) in &updates {
                let json_vec = serde_json::to_string(vector).unwrap_or_default();
                tx.execute(
                    "INSERT OR REPLACE INTO memory_embeddings_768 (id, workspace_id, embedding) VALUES (?1, ?2, vec_f32(?3))",
                    params![id, workspace_id, json_vec],
                )?;

                tx.execute(
                    "UPDATE memory_records SET embedding = ?1, embedding_status = 'completed', updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
                    params![blob, id],
                )?;
            }

            // Save checkpoint
            tx.execute(
                "INSERT OR REPLACE INTO backfill_checkpoint (key, value, updated_at) VALUES ('last_processed_rowid', ?1, CURRENT_TIMESTAMP)",
                params![batch_last_rowid.to_string()],
            )?;

            tx.commit()?;
        }

        last_processed_rowid = batch_last_rowid;
        processed_count += batch_len;

        // Log progress, throughput, ETA
        let elapsed = start_time.elapsed().as_secs_f64();
        let rate = if elapsed > 0.0 {
            processed_count as f64 / elapsed
        } else {
            0.0
        };
        let remaining = (total_remaining as usize).saturating_sub(processed_count);
        let eta_secs = if rate > 0.0 {
            remaining as f64 / rate
        } else {
            0.0
        };

        info!(
            "Batch completed ({} records). Total progress: {}/{} ({:.1}%). Throughput: {:.1} recs/sec. ETA: {:.0}s",
            batch_len,
            processed_count,
            total_remaining,
            (processed_count as f64 / total_remaining as f64) * 100.0,
            rate,
            eta_secs
        );

        // Rate limit: 100ms between batches
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    info!(
        "Backfill binary finished. Total records processed in session: {}",
        processed_count
    );
    Ok(())
}
