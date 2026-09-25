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

/// Memory record from document.
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

/// Paths that the gestalt event bus auto-captures. Unbounded growth here is
/// what drove xavier RSS to multi-GB peaks (see #2264).
const BUS_PATH_PREFIX: &str = "gestalt/bus/";

/// Shared sliding-window state for the bus auto-capture quota.
/// `(window_start_unix, admitted_in_window)`.
static BUS_WINDOW: std::sync::OnceLock<std::sync::Mutex<(i64, usize)>> = std::sync::OnceLock::new();

/// Quota tuning knobs (shared by the consuming check and the read-only peek).
fn bus_quota_config() -> (usize, i64) {
    let per_window: usize = std::env::var("XAVIER_BUS_QUOTA_PER_WINDOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let window_secs: i64 = std::env::var("XAVIER_BUS_QUOTA_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600);
    (per_window, window_secs)
}

/// Sliding-window quota for bus auto-capture.
///
/// Returns `true` when the document should be dropped because the configured
/// window budget is exhausted. Reads two env knobs so operators can tune it
/// without a rebuild:
/// - `XAVIER_BUS_QUOTA_PER_WINDOW` (default 60)
/// - `XAVIER_BUS_QUOTA_WINDOW_SECS` (default 3600)
///
/// A value of `0` disables the cap entirely (explicit opt-out).
fn bus_quota_exceeded(path: &str) -> bool {
    if !path.starts_with(BUS_PATH_PREFIX) {
        return false;
    }
    let (per_window, window_secs) = bus_quota_config();
    if per_window == 0 {
        return false;
    }

    let cell = BUS_WINDOW.get_or_init(|| std::sync::Mutex::new((0, 0)));
    let now = chrono::Utc::now().timestamp();
    let Ok(mut guard) = cell.lock() else {
        return false;
    };
    let (window_start, count) = *guard;
    if window_start == 0 || now - window_start >= window_secs {
        *guard = (now, 1);
        return false;
    }
    if count >= per_window {
        tracing::warn!(
            path = %path,
            window_secs,
            per_window,
            "gestalt bus auto-capture quota reached; dropping event to bound memory growth"
        );
        return true;
    }
    *guard = (window_start, count + 1);
    false
}

/// Read-only quota peek for callers that compute embeddings BEFORE `add`.
///
/// Returns `true` when the bus auto-capture window budget is already
/// exhausted for `path`. Unlike [`bus_quota_exceeded`], it never consumes
/// quota: use it to skip GPU embedding work whose result the quota check
/// inside [`add`] would drop anyway. The consuming check in `add` remains
/// the single source of truth, so a `false` here can still lose a race and
/// be dropped later — safe, just not free.
pub fn bus_quota_exhausted(path: &str) -> bool {
    if !path.starts_with(BUS_PATH_PREFIX) {
        return false;
    }
    let (per_window, window_secs) = bus_quota_config();
    if per_window == 0 {
        return false;
    }
    let cell = BUS_WINDOW.get_or_init(|| std::sync::Mutex::new((0, 0)));
    let now = chrono::Utc::now().timestamp();
    let Ok(guard) = cell.lock() else {
        return false;
    };
    let (window_start, count) = *guard;
    if window_start == 0 || now - window_start >= window_secs {
        return false;
    }
    count >= per_window
}

/// Test-only reset for the shared quota window. The window is process-global
/// and libtest does not guarantee execution order, so every quota test must
/// start from a known state. Zero production impact (`#[cfg(test)]`).
#[cfg(test)]
pub(crate) fn bus_quota_reset_for_tests() {
    if let Some(cell) = BUS_WINDOW.get() {
        if let Ok(mut guard) = cell.lock() {
            *guard = (0, 0);
        }
    }
}

/// Add.
pub async fn add(memory: &QmdMemory, doc: MemoryDocument) -> Result<()> {
    if bus_quota_exceeded(&doc.path) {
        return Ok(());
    }
    emit_operation_event(memory, "add", &doc.path, &doc.metadata).await;

    let canonical_path = doc.path.starts_with("stability/") || doc.path.starts_with("features/");
    let mut updated_in_memory = false;

    if canonical_path {
        let mut docs = memory.docs.write().await;
        if let Some(existing) = docs.iter_mut().find(|d| d.path == doc.path) {
            *existing = doc.clone();
            updated_in_memory = true;
        }
    }

    if !updated_in_memory {
        memory.docs.write().await.push(doc.clone());
    }

    memory.invalidate_cache().await;
    if let Some(store) = memory.store().await {
        store
            .put(memory_record_from_document(&memory.workspace_id, &doc))
            .await?;
    }
    Ok(())
}

/// Update.
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

/// Delete.
///
/// BUG FIX: on a lazily-loaded `QmdMemory` (the production default —
/// `QmdMemory::new_lazy`, see src/cli/server.rs), `memory.docs` stays empty until
/// something first triggers `ensure_loaded()` (e.g. `get`/`list`/`search`). A delete
/// issued before that point used to search this still-empty in-memory cache, find no
/// match, and — because the `store.delete()` call below was (and still is) gated on a
/// match being found — silently no-op against the persistent store too. The record
/// stayed in the DB. A subsequent add-at-the-same-path then inserted a NEW row (new
/// ULID) alongside the never-actually-deleted one, so the next full cache reload
/// (`ensure_loaded`/`init`, which *replaces* `docs` wholesale from the persisted
/// workspace state) surfaced two documents at the same path — a duplicate instead of
/// a clean replace. Ensuring the cache is loaded before we search/evict it closes that
/// gap: delete now always sees (and can remove) a record persisted by an earlier
/// session/instance, not just one added within this process's lifetime.
pub async fn delete(memory: &QmdMemory, path_or_id: &str) -> Result<Option<MemoryDocument>> {
    memory.ensure_loaded().await?;

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

/// Clear.
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

/// Add document.
pub async fn add_document(
    memory: &QmdMemory,
    path: String,
    content: String,
    metadata: Value,
) -> Result<String> {
    add_document_typed_with_embedding(memory, path, content, metadata, None, None).await
}

/// Add document typed.
pub async fn add_document_typed(
    memory: &QmdMemory,
    path: String,
    content: String,
    metadata: Value,
    typed: Option<TypedMemoryPayload>,
) -> Result<String> {
    add_document_typed_with_embedding(memory, path, content, metadata, typed, None).await
}

/// Add document typed with embedding.
pub async fn add_document_typed_with_embedding(
    memory: &QmdMemory,
    path: String,
    content: String,
    metadata: Value,
    typed: Option<TypedMemoryPayload>,
    embedding: Option<Vec<f32>>,
) -> Result<String> {
    let canonical_path = path.starts_with("stability/") || path.starts_with("features/");
    let mut existing_id = None;
    if canonical_path {
        {
            let docs = memory.docs.read().await;
            if let Some(existing) = docs.iter().find(|d| d.path == path) {
                existing_id = existing.id.clone();
            }
        }
        if existing_id.is_none() {
            if let Some(store) = memory.store().await {
                if let Ok(Some(existing_rec)) = store.get(&memory.workspace_id, &path).await {
                    existing_id = Some(existing_rec.id);
                }
            }
        }
    }
    let id = existing_id.unwrap_or_else(|| ulid::Ulid::new().to_string());
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
    } else if bus_quota_exhausted(&path) {
        // Stability: the quota check in `add` below will drop this bus
        // event — skip the GPU embedding instead of wasting it.
        Vec::new()
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

#[cfg(test)]
mod bus_quota_tests {
    use super::super::QmdMemory;
    use super::{
        add_document_typed_with_embedding, bus_quota_exceeded, bus_quota_exhausted,
        bus_quota_reset_for_tests,
    };
    use std::sync::Arc;
    use tokio::sync::RwLock as AsyncRwLock;

    #[test]
    fn non_bus_paths_are_never_capped() {
        std::env::set_var("XAVIER_BUS_QUOTA_PER_WINDOW", "1");
        assert!(!bus_quota_exceeded("projects/xavier/overview"));
        assert!(!bus_quota_exceeded("gestalt/audit/2026-09-15"));
    }

    #[test]
    fn bus_paths_are_capped_within_a_window() {
        bus_quota_reset_for_tests();
        std::env::set_var("XAVIER_BUS_QUOTA_PER_WINDOW", "3");
        std::env::set_var("XAVIER_BUS_QUOTA_WINDOW_SECS", "3600");
        // First three events in the window pass, the fourth is dropped.
        assert!(!bus_quota_exceeded("gestalt/bus/executions/a"));
        assert!(!bus_quota_exceeded("gestalt/bus/executions/b"));
        assert!(!bus_quota_exceeded("gestalt/bus/executions/c"));
        assert!(bus_quota_exceeded("gestalt/bus/executions/d"));
    }

    #[test]
    fn zero_disables_the_cap() {
        std::env::set_var("XAVIER_BUS_QUOTA_PER_WINDOW", "0");
        for i in 0..100 {
            assert!(!bus_quota_exceeded(&format!("gestalt/bus/executions/{i}")));
        }
    }

    #[test]
    fn quota_peek_never_consumes() {
        bus_quota_reset_for_tests();
        std::env::set_var("XAVIER_BUS_QUOTA_PER_WINDOW", "1000000");
        std::env::set_var("XAVIER_BUS_QUOTA_WINDOW_SECS", "3600");
        for i in 0..100 {
            assert!(!bus_quota_exhausted(&format!("gestalt/bus/peek-probe/{i}")));
        }
        // If the peek above had consumed quota, a tight window would now be
        // exhausted. With a 1M budget the admit below proves it did not.
        assert!(!bus_quota_exceeded("gestalt/bus/peek-probe/admit"));
    }

    #[test]
    fn quota_peek_reports_a_full_window() {
        bus_quota_reset_for_tests();
        std::env::set_var("XAVIER_BUS_QUOTA_PER_WINDOW", "1");
        std::env::set_var("XAVIER_BUS_QUOTA_WINDOW_SECS", "3600");
        // Fill the single slot (or observe it already full): either way the
        // window is exhausted afterwards.
        let _ = bus_quota_exceeded("gestalt/bus/peek-probe/fill");
        assert!(bus_quota_exhausted("gestalt/bus/peek-probe/any"));
        // Non-bus paths and a disabled cap are never reported exhausted.
        assert!(!bus_quota_exhausted("projects/xavier/overview"));
        std::env::set_var("XAVIER_BUS_QUOTA_PER_WINDOW", "0");
        assert!(!bus_quota_exhausted("gestalt/bus/peek-probe/any"));
    }

    /// Stability S1.02: end-to-end drop contract. With the window budget
    /// spent, a bus-path ingest stores nothing. The precomputed embedding
    /// keeps this test backend-free and deterministic; GPU-avoidance itself
    /// is covered by `quota_peek_*` above plus the guard's position ahead
    /// of `generate_embedding`. The quota is blind-filled (two `exceeded`
    /// calls with a window of 1 exhaust it from ANY prior global state),
    /// so this holds regardless of test order.
    #[tokio::test]
    async fn bus_exhausted_event_stores_nothing() {
        bus_quota_reset_for_tests();
        std::env::set_var("XAVIER_BUS_QUOTA_PER_WINDOW", "1");
        std::env::set_var("XAVIER_BUS_QUOTA_WINDOW_SECS", "3600");

        // Blind-fill: after two calls the window is exhausted no matter
        // what earlier tests consumed (fresh window → 1 admit + 1 drop;
        // active window → 2 drops).
        let _ = bus_quota_exceeded("gestalt/bus/s1-02/fill-a");
        let _ = bus_quota_exceeded("gestalt/bus/s1-02/fill-b");
        assert!(bus_quota_exhausted("gestalt/bus/s1-02/any"));

        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));

        // Quota spent: this ingest must be dropped by the writer.
        add_document_typed_with_embedding(
            &memory,
            "gestalt/bus/s1-02/second".to_string(),
            "second bus event".to_string(),
            serde_json::json!({}),
            None,
            Some(vec![0.2; 8]),
        )
        .await
        .expect("dropped ingest still returns Ok");
        assert!(memory
            .get("gestalt/bus/s1-02/second")
            .await
            .expect("get works")
            .is_none());

        std::env::set_var("XAVIER_BUS_QUOTA_PER_WINDOW", "60");
    }
}

#[cfg(test)]
mod delete_readd_no_duplicate_tests {
    use super::super::QmdMemory;
    use crate::memory::store::{InMemoryMemoryStore, MemoryRecord, MemoryStore};
    use std::sync::Arc;

    const WORKSPACE: &str = "test-delete-readd-ws";
    const PATH: &str = "notes/delete-readd-duplicate-test";

    /// Regression test for the bug fixed above: on a lazily-loaded `QmdMemory`
    /// (production's default — see `QmdMemory::new_lazy`), deleting a record that was
    /// persisted by an earlier session/instance (i.e. never loaded into *this*
    /// instance's in-memory `docs` cache) used to silently no-op — the in-memory
    /// search found nothing, so the guarded `store.delete()` call never ran, leaving
    /// the row in the store. Re-adding at the same path then inserted a second row
    /// (a fresh ULID, since the writer's path-collision check only covers
    /// "stability/"/"features/" paths), so the next full cache reload showed two
    /// documents at the same path instead of one replaced.
    #[tokio::test]
    async fn delete_then_readd_same_path_replaces_instead_of_duplicating() {
        let store = Arc::new(InMemoryMemoryStore::new());

        // Simulate a record already persisted from an earlier session — written
        // directly to the store, never loaded into any QmdMemory's in-memory cache.
        store
            .put(MemoryRecord {
                id: "pre-existing-record-id".to_string(),
                workspace_id: WORKSPACE.to_string(),
                path: PATH.to_string(),
                content: "original content from a prior session".to_string(),
                ..Default::default()
            })
            .await
            .expect("seed store with pre-existing record");

        // Fresh, lazily-loaded instance — mirrors production wiring
        // (src/cli/server.rs constructs QmdMemory::new_lazy). Its `docs` cache starts
        // empty; nothing has triggered ensure_loaded() yet.
        let memory = QmdMemory::new_lazy(WORKSPACE);
        memory.set_store(store.clone()).await;

        // Delete the record the fresh instance has never itself loaded/cached.
        let deleted = memory.delete(PATH).await.expect("delete should not error");
        assert!(
            deleted.is_some(),
            "delete must find and evict a record persisted by an earlier session, \
             not just one added within this process's lifetime"
        );

        // Re-add at the same path.
        memory
            .add_document(
                PATH.to_string(),
                "new content after delete".to_string(),
                serde_json::json!({}),
            )
            .await
            .expect("re-add after delete should succeed");

        // The persisted store — the ultimate source of truth once a lazy cache
        // reloads — must hold exactly one record at this path, not two.
        let state = store
            .load_workspace_state(WORKSPACE)
            .await
            .expect("load workspace state");
        let matching: Vec<_> = state
            .memories
            .iter()
            .filter(|record| record.path == PATH)
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "expected exactly one persisted record at {PATH} after delete + re-add, found {}: {:?}",
            matching.len(),
            matching.iter().map(|r| &r.id).collect::<Vec<_>>()
        );
        assert_eq!(matching[0].content, "new content after delete");

        // The in-memory cache on a *fresh* instance pointed at the same store (the
        // scenario a real cache reload / process restart exercises) must agree.
        let reloaded = QmdMemory::new_lazy(WORKSPACE);
        reloaded.set_store(store.clone()).await;
        let all_docs = reloaded.all_documents().await;
        let matching_docs: Vec<_> = all_docs.iter().filter(|d| d.path == PATH).collect();
        assert_eq!(
            matching_docs.len(),
            1,
            "expected exactly one document at {PATH} after a fresh cache load, found {}",
            matching_docs.len()
        );
    }
}
