use crate::memory::qmd_memory::types::{CacheMetrics, MemoryDocument, MemoryUsage};
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::store::MemoryRecord;
use anyhow::Result;
use std::sync::atomic::Ordering as AtomicOrdering;

pub fn memory_record_from_document(workspace_id: &str, document: &MemoryDocument) -> MemoryRecord {
    let primary = document
        .metadata
        .get("source_path")
        .and_then(|value| value.as_str())
        .is_none();
    let parent_id = document
        .metadata
        .get("parent_id")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .or_else(|| {
            (!primary)
                .then(|| {
                    document
                        .metadata
                        .get("source_path")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string())
                })
                .flatten()
        });

    MemoryRecord::from_document(workspace_id, document, primary, parent_id)
}

pub async fn init(memory: &QmdMemory) -> Result<()> {
    if let Some(store) = memory.store().await {
        let state = store.load_workspace_state(&memory.workspace_id).await?;
        let docs: Vec<MemoryDocument> = state
            .memories
            .into_iter()
            .map(|record| record.to_document())
            .collect();
        let loaded_memories = docs.len();
        *memory.docs.write().await = docs;
        tracing::info!(
            workspace_id = %memory.workspace_id,
            loaded_memories = loaded_memories,
            "QmdMemory loaded from persistent store"
        );
    }
    Ok(())
}

pub async fn get(memory: &QmdMemory, path_or_id: &str) -> Result<Option<MemoryDocument>> {
    let docs = memory.docs.read().await;
    Ok(docs
        .iter()
        .find(|doc| doc.path == path_or_id || doc.id.as_deref() == Some(path_or_id))
        .cloned())
}

pub async fn add(memory: &QmdMemory, doc: MemoryDocument) -> Result<()> {
    memory.docs.write().await.push(doc.clone());
    memory.invalidate_cache().await;
    if let Some(store) = memory.store().await {
        store
            .put(memory_record_from_document(&memory.workspace_id, &doc))
            .await?;
    }
    Ok(())
}

pub async fn update(memory: &QmdMemory, doc: MemoryDocument) -> Result<()> {
    let persisted = doc.clone();
    let mut docs = memory.docs.write().await;
    if let Some(existing) = docs
        .iter_mut()
        .find(|d| d.id == doc.id || d.path == doc.path)
    {
        *existing = doc;
    } else {
        docs.push(doc);
    }
    drop(docs);
    memory.invalidate_cache().await;
    if let Some(store) = memory.store().await {
        store
            .update(memory_record_from_document(&memory.workspace_id, &persisted))
            .await?;
    }
    Ok(())
}

pub async fn delete(memory: &QmdMemory, path_or_id: &str) -> Result<Option<MemoryDocument>> {
    let mut docs = memory.docs.write().await;
    let removed = docs
        .iter()
        .position(|doc| doc.path == path_or_id || doc.id.as_deref() == Some(path_or_id))
        .map(|index| docs.remove(index));
    drop(docs);

    if removed.is_some() {
        memory.invalidate_cache().await;
        if let Some(store) = memory.store().await {
            let _ = store.delete(&memory.workspace_id, path_or_id).await?;
        }
    }

    Ok(removed)
}

pub async fn clear(memory: &QmdMemory) -> Result<usize> {
    let ids = memory
        .docs
        .read()
        .await
        .iter()
        .filter_map(|doc| doc.id.clone().or_else(|| Some(doc.path.clone())))
        .collect::<Vec<_>>();
    let mut docs = memory.docs.write().await;
    let removed = docs.len();
    docs.clear();
    drop(docs);
    memory.invalidate_cache().await;
    if let Some(store) = memory.store().await {
        for id in ids {
            let _ = store.delete(&memory.workspace_id, &id).await?;
        }
    }
    Ok(removed)
}

pub async fn usage(memory: &QmdMemory) -> MemoryUsage {
    let docs = memory.docs.read().await;
    MemoryUsage {
        document_count: docs.len(),
        storage_bytes: docs.iter().map(MemoryDocument::estimated_bytes).sum(),
    }
}

pub async fn cache_metrics(memory: &QmdMemory) -> CacheMetrics {
    CacheMetrics {
        hits: memory.cache_counters.hits.load(AtomicOrdering::Relaxed),
        misses: memory.cache_counters.misses.load(AtomicOrdering::Relaxed),
        entries: memory.search_cache.read().await.len(),
    }
}

pub async fn export(memory: &QmdMemory, public_only: bool) -> Result<Vec<MemoryDocument>> {
    let docs = memory.docs.read().await;
    let exported = docs
        .iter()
        .filter(|doc| {
            if !public_only {
                return true;
            }
            // If public_only, check metadata for visibility
            let is_private = doc
                .metadata
                .get("is_private")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let visibility = doc
                .metadata
                .get("visibility")
                .and_then(|v| v.as_str())
                .unwrap_or("public");

            !is_private && visibility != "private"
        })
        .cloned()
        .collect();
    Ok(exported)
}
