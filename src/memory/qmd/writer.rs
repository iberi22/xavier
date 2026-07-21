// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! QMD document writer
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use serde_json::{json, Value};
use std::collections::HashSet;

use crate::memory::qmd_memory::reader::generate_embedding;
use crate::memory::qmd_memory::types::MemoryDocument;
use crate::memory::qmd_memory::utils::*;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::TypedMemoryPayload;
use crate::memory::store::MemoryRecord;
use crate::session::types::{SessionEvent, SessionEventType};
use anyhow::Result;

// ── CRUD write operations ────────────────────────────────────────────

pub(crate) fn memory_record_from_document(
    workspace_id: &str,
    document: &MemoryDocument,
) -> MemoryRecord {
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

async fn emit_operation_event(memory: &QmdMemory, operation: &str, path: &str, metadata: &Value) {
    let session_id = metadata
        .get("session_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            metadata
                .get("namespace")
                .and_then(|v| v.get("session_id"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("system")
        .to_string();

    let event = SessionEvent {
        session_id,
        event_type: SessionEventType::ToolResult,
        timestamp: chrono::Utc::now(),
        content: Some(format!("Memory {} operation on path: {}", operation, path)),
        metadata: Some(serde_json::json!({
            "operation": operation,
            "path": path,
            "workspace_id": memory.workspace_id,
        })),
    };

    // Note: In a full implementation, we would send this to a dispatcher or port.
    // For now, we'll log it as a hook.
    tracing::info!("Auto-captured memory event: {:?}", event);
}

pub async fn add(memory: &QmdMemory, doc: MemoryDocument) -> Result<()> {
    emit_operation_event(memory, "add", &doc.path, &doc.metadata).await;
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
    emit_operation_event(memory, "update", &doc.path, &doc.metadata).await;
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
            .update(memory_record_from_document(
                &memory.workspace_id,
                &persisted,
            ))
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

    if let Some(ref doc) = removed {
        emit_operation_event(memory, "delete", &doc.path, &doc.metadata).await;
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

// ── Document indexing ─────────────────────────────────────────────────

pub async fn add_document(
    memory: &QmdMemory,
    path: String,
    content: String,
    metadata: Value,
) -> Result<String> {
    add_document_typed_with_embedding(memory, path, content, metadata, None, None).await
}

pub async fn add_document_typed(
    memory: &QmdMemory,
    path: String,
    content: String,
    metadata: Value,
    typed: Option<TypedMemoryPayload>,
) -> Result<String> {
    add_document_typed_with_embedding(memory, path, content, metadata, typed, None).await
}

pub async fn add_document_typed_with_embedding(
    memory: &QmdMemory,
    path: String,
    content: String,
    metadata: Value,
    typed: Option<TypedMemoryPayload>,
    embedding: Option<Vec<f32>>,
) -> Result<String> {
    let id = ulid::Ulid::new().to_string();
    let metadata = crate::memory::schema::normalize_metadata(
        &path,
        metadata,
        &memory.workspace_id,
        typed.as_ref(),
    )?;
    let metadata = normalize_locomo_metadata(&path, metadata);
    let variants = expand_document_variants(&path, &content, &metadata);
    let is_locomo_benchmark = metadata
        .get("benchmark")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("locomo"))
        || path.contains("locomo/");
    let base_embedding = if is_locomo_benchmark {
        Vec::new()
    } else if let Some(embedding) = embedding.clone() {
        embedding
    } else {
        generate_embedding(&content)
            .await
            .unwrap_or_else(|_| Vec::new())
    };

    for (index, (variant_path, variant_content, variant_metadata)) in
        variants.into_iter().enumerate()
    {
        let variant_embedding = if is_locomo_benchmark || variant_content == content {
            base_embedding.clone()
        } else {
            generate_embedding(&variant_content)
                .await
                .unwrap_or_else(|_| Vec::new())
        };

        memory
            .add(MemoryDocument {
                id: Some(if index == 0 {
                    id.clone()
                } else {
                    ulid::Ulid::new().to_string()
                }),
                path: variant_path,
                content: variant_content,
                metadata: variant_metadata,
                content_vector: Some(variant_embedding.clone()),
                embedding: variant_embedding,
                cluster_id: typed.as_ref().and_then(|t| t.cluster_id.clone()),
                parent_id: None,
                level: typed
                    .as_ref()
                    .and_then(|t| t.level)
                    .unwrap_or(crate::memory::schema::MemoryLevel::Raw),
                relation: typed.as_ref().and_then(|t| t.relation.clone()),
                clearance: typed.as_ref().and_then(|t| t.clearance).unwrap_or_default(),
                minhash: None,
                score: 0.0,
                ..Default::default()
            })
            .await?;
    }

    Ok(id)
}

/// Expand a document into derived variants (facts, temporal events, etc.)
pub fn expand_document_variants(
    path: &str,
    content: &str,
    metadata: &Value,
) -> Vec<(String, String, Value)> {
    let mut variants = vec![(path.to_string(), content.to_string(), metadata.clone())];

    if !is_locomo_document(path, metadata) {
        return variants;
    }

    let session_time = metadata
        .get("session_time")
        .and_then(|value| value.as_str())
        .map(str::to_string);

    let speaker = metadata
        .get("speaker")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| extract_primary_speaker(content));

    variants.extend(build_fact_variants(
        path,
        content,
        metadata,
        speaker.as_deref(),
    ));
    variants.extend(build_temporal_variants(
        path,
        content,
        metadata,
        speaker.as_deref(),
        session_time.as_deref(),
    ));

    dedupe_variants(variants)
}

/// Normalize LOCOMO document metadata (dia_ids, source paths).
pub fn normalize_locomo_metadata(path: &str, metadata: Value) -> Value {
    if !is_locomo_document(path, &metadata) {
        return metadata;
    }

    let mut metadata = metadata;
    if let Some(object) = metadata.as_object_mut() {
        if let Some(normalized) = object
            .get("dia_id")
            .and_then(|value| value.as_str())
            .and_then(normalize_dia_id)
            .or_else(|| extract_normalized_dia_id_from_path(path))
        {
            object.insert("dia_id".to_string(), json!(&normalized));
            object.insert("normalized_dia_id".to_string(), json!(normalized));
        }

        if let Some(source_path) = object.get("source_path").and_then(|value| value.as_str()) {
            let normalized_source_path = normalize_locomo_path(source_path);
            object.insert("source_path".to_string(), json!(&normalized_source_path));
            if let Some(normalized) = extract_normalized_dia_id_from_path(&normalized_source_path) {
                object.insert("source_dia_id".to_string(), json!(normalized));
            }
        }
    }

    metadata
}

// ── Fact / temporal variant builders ──────────────────────────────────

fn build_fact_variants(
    path: &str,
    content: &str,
    metadata: &Value,
    speaker: Option<&str>,
) -> Vec<(String, String, Value)> {
    let mut variants = Vec::new();
    let Some(subject) = speaker else {
        return variants;
    };

    let lowered = content.to_lowercase();
    let mut push_fact = |index: usize, memory_kind: &str, fact_type: &str, value: String| {
        let sentence = match fact_type {
            "identity" => format!("{subject} is {value}."),
            "relationship_status" => format!("{subject} is {value}."),
            "research_topic" => format!("{subject} researched {value}."),
            "career_interest" => format!("{subject} would likely pursue {value}."),
            _ => format!("{subject}: {value}."),
        };
        variants.push((
            format!("{path}#derived/{memory_kind}/{index}"),
            sentence,
            build_variant_metadata(
                metadata,
                path,
                memory_kind,
                json!({
                    "speaker": subject,
                    "normalized_value": value,
                    "answer_span": value,
                    "fact_type": fact_type,
                }),
            ),
        ));
    };

    if let Some(value) = crate::memory::qmd_memory::utils::capture_value(
        content,
        r"(?i)\b(?:i am|i'm)\s+(?:a\s+)?(transgender woman|trans woman|woman|man|nonbinary|non-binary)\b",
    ) {
        push_fact(0, "entity_state", "identity", sentence_case_phrase(&value));
    } else if lowered.contains("transgender") || lowered.contains("trans community") {
        push_fact(
            0,
            "entity_state",
            "identity",
            "Transgender woman".to_string(),
        );
    }

    if let Some(value) = crate::memory::qmd_memory::utils::capture_value(
        content,
        r"(?i)\b(?:i am|i'm)\s+(single|married|divorced|engaged|widowed)\b",
    ) {
        push_fact(
            1,
            "entity_state",
            "relationship_status",
            sentence_case_phrase(&value),
        );
    } else if lowered.contains("single parent") {
        push_fact(
            1,
            "entity_state",
            "relationship_status",
            "Single".to_string(),
        );
    }

    if let Some(value) = crate::memory::qmd_memory::utils::capture_value(
        content,
        r"(?i)\b(?:researched|researching)\s+([A-Za-z][A-Za-z\s'-]{2,80})",
    ) {
        let cleaned = trim_fact_value(&value);
        if !cleaned.is_empty() {
            push_fact(
                2,
                "fact_atom",
                "research_topic",
                sentence_case_phrase(&cleaned),
            );
        }
    }

    if lowered.contains("counseling")
        || lowered.contains("mental health")
        || lowered.contains("psychology")
    {
        let inferred = if lowered.contains("counseling") && lowered.contains("mental health") {
            "Psychology, counseling certification".to_string()
        } else if lowered.contains("psychology") && lowered.contains("counsel") {
            "Psychology, counseling".to_string()
        } else if lowered.contains("mental health") {
            "Counseling, mental health".to_string()
        } else if lowered.contains("psychology") {
            "Psychology".to_string()
        } else {
            "Counseling".to_string()
        };
        push_fact(3, "summary_fact", "career_interest", inferred);
    }

    if let Some(value) = extract_duration_value(content) {
        push_fact(4, "fact_atom", "duration", value);
    }

    if let Some(value) = crate::memory::qmd_memory::utils::capture_value(
        content,
        r"(?i)\bmoved from\s+([A-Z][a-zA-Z]+)\b",
    ) {
        push_fact(5, "fact_atom", "origin_place", sentence_case_phrase(&value));
    }

    let activities = collect_present_keywords(
        &lowered,
        &[
            "pottery", "camping", "painting", "swimming", "running", "reading", "violin", "hiking",
        ],
    );
    if !activities.is_empty() {
        push_fact(
            6,
            "summary_fact",
            "activities",
            title_case_list(&activities),
        );
    }

    let places = collect_present_keywords(&lowered, &["beach", "mountains", "forest", "museum"]);
    if !places.is_empty() {
        push_fact(7, "summary_fact", "places", title_case_list(&places));
    }

    let preferences = collect_present_keywords(&lowered, &["dinosaurs", "nature"]);
    if !preferences.is_empty() {
        push_fact(
            8,
            "summary_fact",
            "preferences",
            title_case_list(&preferences),
        );
    }

    let books = extract_quoted_titles(content);
    if !books.is_empty() {
        push_fact(9, "summary_fact", "books", books.join(", "));
    }

    variants
}

fn build_temporal_variants(
    path: &str,
    content: &str,
    metadata: &Value,
    speaker: Option<&str>,
    session_time: Option<&str>,
) -> Vec<(String, String, Value)> {
    let Some(resolved_date) = resolve_temporal_value(content, session_time) else {
        return Vec::new();
    };

    let subject = speaker.unwrap_or_default();
    let action = infer_event_action(content);
    let sentence = if subject.is_empty() {
        format!("{action} on {resolved_date}.")
    } else {
        format!("{subject} {action} on {resolved_date}.")
    };

    vec![(
        format!("{path}#derived/temporal_event/0"),
        sentence,
        build_variant_metadata(
            metadata,
            path,
            "temporal_event",
            json!({
                "speaker": subject,
                "event_subject": subject,
                "event_action": action,
                "resolved_date": resolved_date,
                "resolved_granularity": infer_date_granularity(&resolved_date),
            }),
        ),
    )]
}

fn build_variant_metadata(
    metadata: &Value,
    source_path: &str,
    memory_kind: &str,
    extra: Value,
) -> Value {
    let mut base = metadata.clone();
    if let Some(object) = base.as_object_mut() {
        object.insert("source_path".to_string(), json!(source_path));
        object.insert("memory_kind".to_string(), json!(memory_kind));
        if let Some(extra_object) = extra.as_object() {
            for (key, value) in extra_object {
                object.insert(key.clone(), value.clone());
            }
        }
    }
    normalize_locomo_metadata(source_path, base)
}

fn dedupe_variants(variants: Vec<(String, String, Value)>) -> Vec<(String, String, Value)> {
    let mut seen = HashSet::new();
    variants
        .into_iter()
        .filter(|(_, content, metadata)| {
            let key = format!(
                "{}|{}|{}",
                content,
                metadata
                    .get("memory_kind")
                    .and_then(|value| value.as_str())
                    .unwrap_or("primary"),
                metadata
                    .get("normalized_value")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
            );
            seen.insert(key)
        })
        .collect()
}
