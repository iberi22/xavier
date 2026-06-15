//! Reindex memory embeddings into sqlite-vec.

use anyhow::{anyhow, Context, Result};
use clap::Args;
use rusqlite::{params, Connection};
use std::collections::HashSet;

use xavier::memory::embedder::EmbeddingClient;
use xavier::memory::sqlite_vec_store::{vector, VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::MemoryStore;

#[derive(Args, Debug, Clone)]
pub struct ReindexArgs {
    /// Number of memories to process per progress batch
    #[arg(long, default_value_t = 100)]
    pub batch_size: usize,
    /// Recompute embeddings even when sqlite-vec already has one
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone)]
struct ReindexCandidate {
    id: String,
    workspace_id: String,
    content: String,
    encrypted: bool,
}

#[derive(Debug, Default)]
struct ReindexReport {
    scanned: usize,
    indexed: usize,
    failed: usize,
    skipped_empty: usize,
}

pub async fn handle_reindex(args: ReindexArgs) -> Result<()> {
    let batch_size = args.batch_size.max(1);
    configure_openrouter_defaults();

    let config = VecSqliteStoreConfig::from_env();
    vector::register_sqlite_vec_extension()?;
    let store = VecSqliteMemoryStore::from_env()
        .await
        .context("failed to initialize sqlite-vec memory store")?;

    println!("Reindexing memory embeddings in {}", config.path.display());
    println!(
        "Embedding provider: OpenRouter-compatible text-embedding-3-small{}",
        if args.force { " (force)" } else { "" }
    );

    let candidates = load_candidates(config.clone(), args.force)
        .await
        .context("failed to query memories for reindex")?;

    if candidates.is_empty() {
        println!("No memories need reindexing.");
        return Ok(());
    }

    println!(
        "Found {} candidate memories. Processing in batches of {}...",
        candidates.len(),
        batch_size
    );

    let provider = EmbeddingClient::from_env_async()
        .await
        .context("failed to initialize embedding provider")?;

    let mut report = ReindexReport {
        scanned: candidates.len(),
        ..Default::default()
    };

    for (batch_idx, batch) in candidates.chunks(batch_size).enumerate() {
        let batch_start = batch_idx * batch_size;
        println!(
            "Batch {}: memories {}-{} of {}",
            batch_idx + 1,
            batch_start + 1,
            (batch_start + batch.len()).min(candidates.len()),
            candidates.len()
        );

        for candidate in batch {
            let content = resolve_candidate_content(&store, candidate).await?;
            if content.trim().is_empty() {
                report.skipped_empty += 1;
                continue;
            }

            match provider.embed(&content).await {
                Ok(embedding) if !embedding.is_empty() => {
                    upsert_embedding(config.clone(), candidate.clone(), embedding)
                        .await
                        .with_context(|| {
                            format!("failed to save embedding for {}", candidate.id)
                        })?;
                    report.indexed += 1;
                }
                Ok(_) => {
                    report.failed += 1;
                    eprintln!(
                        "Embedding provider returned an empty vector for {}",
                        candidate.id
                    );
                }
                Err(error) => {
                    report.failed += 1;
                    eprintln!("Failed to embed {}: {}", candidate.id, error);
                }
            }
        }

        println!(
            "Progress: {}/{} indexed, {} failed, {} skipped empty",
            report.indexed, report.scanned, report.failed, report.skipped_empty
        );
    }

    println!("Reindex complete.");
    println!("Scanned: {}", report.scanned);
    println!("Indexed: {}", report.indexed);
    println!("Failed: {}", report.failed);
    println!("Skipped empty: {}", report.skipped_empty);

    if report.failed > 0 {
        return Err(anyhow!(
            "reindex finished with {} failed memories",
            report.failed
        ));
    }

    Ok(())
}

fn configure_openrouter_defaults() {
    if std::env::var("XAVIER_EMBEDDING_PROVIDER_MODE").is_err() {
        std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
    }

    if std::env::var("XAVIER_EMBEDDING_URL").is_err() {
        std::env::set_var(
            "XAVIER_EMBEDDING_URL",
            "https://openrouter.ai/api/v1/embeddings",
        );
    }

    if std::env::var("XAVIER_EMBEDDING_MODEL").is_err() {
        std::env::set_var("XAVIER_EMBEDDING_MODEL", "text-embedding-3-small");
    }

    if std::env::var("OPENAI_API_KEY").is_err() {
        if let Ok(openrouter_key) = std::env::var("OPENROUTER_API_KEY") {
            std::env::set_var("OPENAI_API_KEY", openrouter_key);
        }
    }
}

async fn load_candidates(
    config: VecSqliteStoreConfig,
    force: bool,
) -> Result<Vec<ReindexCandidate>> {
    tokio::task::spawn_blocking(move || {
        vector::register_sqlite_vec_extension()?;
        let conn = Connection::open(&config.path)
            .with_context(|| format!("failed to open {}", config.path.display()))?;

        let sql = if force {
            "SELECT id, workspace_id, content, encrypted_dek
             FROM memory_records
             ORDER BY updated_at ASC, id ASC"
        } else {
            "SELECT m.id, m.workspace_id, m.content, m.encrypted_dek
             FROM memory_records m
             LEFT JOIN memory_embeddings e
               ON e.id = m.id AND e.workspace_id = m.workspace_id
             WHERE e.id IS NULL
             ORDER BY m.updated_at ASC, m.id ASC"
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(ReindexCandidate {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                content: row.get(2)?,
                encrypted: row.get::<_, Option<Vec<u8>>>(3)?.is_some(),
            })
        })?;

        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for row in rows {
            let candidate = row?;
            let key = (candidate.workspace_id.clone(), candidate.id.clone());
            if seen.insert(key) {
                candidates.push(candidate);
            }
        }

        Ok(candidates)
    })
    .await
    .context("reindex candidate query task failed")?
}

async fn resolve_candidate_content(
    store: &VecSqliteMemoryStore,
    candidate: &ReindexCandidate,
) -> Result<String> {
    if !candidate.encrypted {
        return Ok(candidate.content.clone());
    }

    let record = store
        .get(&candidate.workspace_id, &candidate.id)
        .await?
        .ok_or_else(|| anyhow!("encrypted memory {} was not found", candidate.id))?;

    Ok(record.content)
}

async fn upsert_embedding(
    config: VecSqliteStoreConfig,
    candidate: ReindexCandidate,
    embedding: Vec<f32>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        vector::register_sqlite_vec_extension()?;
        let conn = Connection::open(&config.path)
            .with_context(|| format!("failed to open {}", config.path.display()))?;
        let embedding_json =
            serde_json::to_string(&embedding).context("failed to serialize embedding")?;

        conn.execute(
            "INSERT OR REPLACE INTO memory_embeddings(id, workspace_id, embedding)
             VALUES (?1, ?2, vec_f32(?3))",
            params![candidate.id, candidate.workspace_id, embedding_json],
        )?;

        Ok(())
    })
    .await
    .context("reindex embedding upsert task failed")?
}
