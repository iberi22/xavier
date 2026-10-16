//! Keeps the QMD search index in step with records applied from a peer.
//!
//! `/memory/search` reads the QMD index, not the record store, so every path
//! that writes received records to the store has to mirror them here.

use crate::memory::qmd_memory::{reader::generate_embedding, MemoryDocument, QmdMemory};
use crate::memory::store::{MemoryRecord, MemoryStore};

use super::{ChunkDiff, DiffAction};

/// Gives `doc` an embedding when it has none. A vector the document already
/// carries (a received record usually has one) is kept.
pub async fn embed_if_missing(doc: &mut MemoryDocument) {
    if !doc.embedding.is_empty() {
        return;
    }
    if let Ok(vector) = generate_embedding(&doc.content).await {
        if !vector.is_empty() {
            doc.embedding = vector;
        }
    }
}

/// Mirrors the changes that were just applied to `store` into `qmd`.
///
/// Only records of `workspace_id` are indexed; a record of another workspace
/// stays in the store and never reaches this index. An Add/Update re-reads the
/// row the store now holds, because last-writer-wins may have kept the local
/// copy, and the index entry takes that row's id and content. The index
/// write never touches the store (no revision bump, no echo) and replaces an existing entry for the same path, so an update does not leave
/// the old version beside the new one. A Delete removes the path from the
/// index. Failures are logged; the caller's apply has already succeeded.
pub async fn index_applied_changes(
    store: &dyn MemoryStore,
    qmd: &QmdMemory,
    workspace_id: &str,
    diffs: &[ChunkDiff],
) {
    for diff in diffs {
        match diff.action {
            DiffAction::Add | DiffAction::Update => {
                let Some(data) = &diff.data else {
                    continue;
                };
                let Ok(incoming) = serde_json::from_slice::<MemoryRecord>(data) else {
                    continue;
                };
                if incoming.workspace_id != workspace_id {
                    continue;
                }
                let stored = match store.get(workspace_id, &incoming.path).await {
                    Ok(Some(record)) => record,
                    Ok(None) => continue,
                    Err(error) => {
                        tracing::warn!(error = %error, path = %incoming.path, "sync: re-read failed");
                        continue;
                    }
                };
                let mut doc = stored.to_document();
                embed_if_missing(&mut doc).await;
                if let Err(error) = qmd.index_upsert(doc).await {
                    tracing::warn!(error = %error, memory_id = %stored.id, "sync: qmd index failed");
                }
            }
            DiffAction::Delete => {
                if diff.namespace != workspace_id {
                    continue;
                }
                let Some(path) = &diff.record_path else {
                    continue;
                };
                // Index-only: the store delete was already applied by the merge.
                if let Err(error) = qmd.index_remove(path).await {
                    tracing::warn!(error = %error, path = %path, "sync: qmd delete failed");
                }
            }
        }
    }
}
