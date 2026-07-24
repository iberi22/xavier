//! QmdMemory adapter that implements MemoryQueryPort.
//! Wraps QmdMemory (the domain) behind the inbound port interface.
/// NOTE: HexArch improvement — depends on concrete crate::memory::qmd_memory, should use a port abstraction
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::store::MemoryRecord;
use crate::ports::inbound::MemoryQueryPort;
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Clone)]
pub struct QmdMemoryAdapter {
    inner: Arc<QmdMemory>,
}

impl QmdMemoryAdapter {
    /// New.
    pub fn new(inner: Arc<QmdMemory>) -> Self {
        Self { inner }
    }
}

/// Compute cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum();
    let mag_b: f32 = b.iter().map(|x| x * x).sum();
    let norm = (mag_a * mag_b).sqrt();
    if norm < f32::EPSILON { 0.0 } else { dot / norm }
}


#[async_trait]
impl MemoryQueryPort for QmdMemoryAdapter {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        let results = self
            .inner
            .search_filtered(query, limit, filters.as_ref())
            .await?;

        let workspace_id = self.inner.workspace_id();
        Ok(results
            .into_iter()
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .collect())
    }

    async fn add(&self, record: MemoryRecord) -> anyhow::Result<String> {
        // Check for semantic dedup mode
        let dedup_mode = record.metadata.get("_dedup_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("off");

        if dedup_mode == "semantic" {
            // Generate embedding for the new content
            let embedding = crate::memory::qmd::reader::generate_embedding(&record.content).await?;
            
            // If we have a valid embedding, search for duplicates
            if !embedding.is_empty() {
                let all_docs = self.inner.all_documents().await;
                let path_prefix = std::path::Path::new(&record.path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                
                let mut best_match: Option<(f32, crate::memory::qmd::types::MemoryDocument)> = None;
                
                for doc in &all_docs {
                    // Only compare documents with same path prefix
                    if !doc.path.starts_with(&path_prefix) && !path_prefix.is_empty() {
                        continue;
                    }
                    if let Some(ref doc_emb) = doc.content_vector {
                        if !doc_emb.is_empty() {
                            let sim = cosine_similarity(&embedding, doc_emb);
                            if sim > 0.90 {
                                let is_better = best_match
                                    .as_ref()
                                    .map(|(best_sim, _)| sim > *best_sim)
                                    .unwrap_or(true);
                                if is_better {
                                    best_match = Some((sim, doc.clone()));
                                }
                            }
                        }
                    }
                }

                if let Some((_, matched_doc)) = best_match {
                    // High similarity — update existing doc instead of inserting
                    let mut updated_doc = matched_doc.clone();
                    updated_doc.content = record.content.clone();
                    updated_doc.content_vector = Some(embedding.clone());
                    updated_doc.embedding = embedding;
                    if let Some(meta_obj) = updated_doc.metadata.as_object_mut() {
                        meta_obj.insert("updated_at".to_string(), serde_json::json!(chrono::Utc::now().to_rfc3339()));
                        meta_obj.remove("_dedup_mode");
                    }
                    self.inner.update(updated_doc).await?;
                    return Ok(matched_doc.id.unwrap_or_else(|| ulid::Ulid::new().to_string()));
                }
            }
        }

        // Default path — insert new document
        let doc = record.to_document();
        self.inner
            .add_document(doc.path, doc.content, doc.metadata)
            .await
    }

    async fn update(&self, id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord> {
        let mut doc = record.to_document();
        doc.id = Some(id.to_string());
        self.inner.update(doc).await?;
        let workspace_id = self.inner.workspace_id();
        let result = self.inner.get(id).await?;
        result
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .ok_or_else(|| anyhow::anyhow!("Record not found after update"))
    }

    async fn delete(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        let workspace_id = self.inner.workspace_id();
        let result = self.inner.delete(id).await?;
        Ok(result.map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None)))
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        let workspace_id = self.inner.workspace_id();
        let result = self.inner.get(id).await?;
        Ok(result.map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None)))
    }

    async fn list(&self, workspace_id: &str, limit: usize) -> anyhow::Result<Vec<MemoryRecord>> {
        let limit = limit.clamp(1, 100);
        let results = self.inner.all_documents().await;

        Ok(results
            .into_iter()
            .take(limit)
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .collect())
    }

    async fn export(&self, public_only: bool) -> anyhow::Result<Vec<MemoryRecord>> {
        let workspace_id = self.inner.workspace_id();
        let results = self.inner.export(public_only).await?;

        Ok(results
            .into_iter()
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .collect())
    }

    async fn ls(&self, path: &str) -> anyhow::Result<Vec<crate::memory::qmd::types::NavEntry>> {
        self.inner.ls(path).await
    }

    async fn expand_depth(
        &self,
        results: &[MemoryRecord],
        depth: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        let docs: Vec<_> = results.iter().map(|r| r.to_document()).collect();
        let expanded = self
            .inner
            .expand_depth(&docs, depth, filters.as_ref())
            .await?;
        let workspace_id = self.inner.workspace_id();
        Ok(expanded
            .into_iter()
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .collect())
    }
}
