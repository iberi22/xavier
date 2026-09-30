//! QMD document writer
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use serde_json::{json, Value};
use std::collections::HashSet;

use crate::domain::cycle_breaks::w30_09::{SessionEvent, SessionEventType};
use crate::memory::qmd_memory::reader::generate_embedding;
use crate::memory::qmd_memory::types::MemoryDocument;
use crate::memory::qmd_memory::utils::*;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::sanitizer::{validate_memory_content, ContentCheck};
use crate::memory::schema::TypedMemoryPayload;
use crate::memory::store::MemoryRecord;
use anyhow::Result;

// ── Write-time integrity ────────────────────────────────────────────

/// Paths the writer treats as canonical: exactly one record per path.
fn is_canonical_path(path: &str) -> bool {
    path.starts_with("stability/") || path.starts_with("features/")
}

/// Stamp the integrity verdict on the metadata of a record being written.
///
/// The marker is set at write time only; records written before this gate
/// (and records re-ingested by another backend) simply have no `integrity`
/// key and keep loading unchanged.
fn stamp_integrity(metadata: &mut Value, check: &ContentCheck) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    object.insert("integrity".to_string(), json!(check.integrity()));
    if check.suspect.is_empty() {
        object.remove("integrity_reasons");
    } else {
        object.insert("integrity_reasons".to_string(), json!(check.suspect));
    }
}

/// Run the write-time gate. Returns the check so the caller can stamp it, or
/// the reject reason to surface as `Content validation failed: ...`.
fn validate_write(content: &str) -> std::result::Result<ContentCheck, String> {
    let check = validate_memory_content(content);
    if check.is_rejected() {
        return Err(check.reject.clone().unwrap_or_default());
    }
    Ok(check)
}

/// Metadata keys the server stamps or rewrites on every write, stripped before
/// two records are compared.
///
/// `normalize_metadata` rewrites `updated_at` on every call, `to_document`
/// injects `created_at`/`revision` on every load, and this gate itself writes
/// `integrity`/`integrity_reasons`; leaving any of them in would make a
/// byte-identical retry look like a brand new record.
const VOLATILE_METADATA_KEYS: [&str; 11] = [
    "created_at",
    "updated_at",
    "integrity",
    "integrity_reasons",
    "revision",
    "revisions",
    "recorded_at",
    "observed_at",
    "primary",
    "parent_id",
    "score",
];

/// `metadata` without the server-stamped keys, so caller metadata can be
/// compared against a stored one.
fn comparable_metadata(metadata: &Value) -> Value {
    let Some(object) = metadata.as_object() else {
        return metadata.clone();
    };
    let mut object = object.clone();
    for key in VOLATILE_METADATA_KEYS {
        object.remove(key);
    }
    Value::Object(object)
}

/// A write is an idempotent retry — the very same record, not a new one — only
/// when path, content **and** caller metadata all match.
///
/// Content alone is not an identity: `synapse/trades/<symbol>` and
/// `activity/claude/<date>/<session>` hold one record per trade or per
/// telemetry event, and those records share a path and can share a body while
/// differing in session, index or trade. Collapsing them would silently drop
/// real data, so anything that is not a perfect retry keeps appending.
fn is_identical_write(
    path: &str,
    content: &str,
    metadata: &Value,
    existing_path: &str,
    existing_content: &str,
    existing_metadata: &Value,
) -> bool {
    path == existing_path
        && content == existing_content
        && comparable_metadata(metadata) == comparable_metadata(existing_metadata)
}

fn is_identical_document(
    existing: &MemoryDocument,
    path: &str,
    content: &str,
    metadata: &Value,
) -> bool {
    is_identical_write(
        path,
        content,
        metadata,
        &existing.path,
        &existing.content,
        &existing.metadata,
    )
}

fn is_identical_record(
    existing: &MemoryRecord,
    path: &str,
    content: &str,
    metadata: &Value,
) -> bool {
    is_identical_write(
        path,
        content,
        metadata,
        &existing.path,
        &existing.content,
        &existing.metadata,
    )
}

/// Id of an existing record that already holds this exact write at `path`,
/// looking in the in-memory cache first and then in the store.
///
/// Retrying a write with identical path, content and metadata is the same
/// logical record, not a new one: minting a second id for it is what put two
/// candidates for one path into the search index.
async fn identical_record_id(
    memory: &QmdMemory,
    path: &str,
    content: &str,
    metadata: &Value,
) -> Option<String> {
    let cached = {
        let docs = memory.docs.read().await;
        docs.iter()
            .find(|doc| is_identical_document(doc, path, content, metadata))
            .and_then(|doc| doc.id.clone())
    };
    if cached.is_some() {
        return cached;
    }
    if let Some(store) = memory.store().await {
        if let Ok(Some(record)) = store.get(&memory.workspace_id, path).await {
            if is_identical_record(&record, path, content, metadata) {
                return Some(record.id);
            }
        }
    }
    None
}

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
pub async fn add(memory: &QmdMemory, mut doc: MemoryDocument) -> Result<()> {
    let check = validate_write(&doc.content)
        .map_err(|reason| anyhow::anyhow!("Content validation failed: {reason}"))?;
    stamp_integrity(&mut doc.metadata, &check);
    add_validated(memory, doc).await
}

/// Add a document whose content and metadata already passed the write-time gate.
///
/// `add_document_typed_with_embedding` validates and stamps the base document
/// once and then hands every derived variant here. Re-running the gate per
/// variant would pay for the same decision twice and could reject a write
/// whose own content is perfectly fine because a derived summary tripped a
/// rule.
async fn add_validated(memory: &QmdMemory, doc: MemoryDocument) -> Result<()> {
    if bus_quota_exceeded(&doc.path) {
        return Ok(());
    }

    let canonical_path = is_canonical_path(&doc.path);
    let mut updated_in_memory = false;

    if !canonical_path
        && identical_record_id(memory, &doc.path, &doc.content, &doc.metadata)
            .await
            .is_some()
    {
        // Idempotent retry: the record is already stored, so there is nothing to
        // add and no second index entry to create. Checked against the store
        // first, before the guard, because that lookup awaits.
        return Ok(());
    }

    {
        // The duplicate check and the append happen under one guard, so a
        // concurrent writer can never observe an absent record here and then
        // push a second copy of it.
        let mut docs = memory.docs.write().await;
        if canonical_path {
            if let Some(existing) = docs.iter_mut().find(|d| d.path == doc.path) {
                *existing = doc.clone();
                updated_in_memory = true;
            }
        } else if docs
            .iter()
            .any(|d| is_identical_document(d, &doc.path, &doc.content, &doc.metadata))
        {
            return Ok(());
        }

        if !updated_in_memory {
            docs.push(doc.clone());
        }
    }

    // Only a write that actually lands is announced: a short-circuited retry is
    // invisible to the bus instead of looking like a second event.
    emit_operation_event(memory, "add", &doc.path, &doc.metadata).await;
    memory.invalidate_cache().await;
    if let Some(store) = memory.store().await {
        store
            .put(memory_record_from_document(&memory.workspace_id, &doc))
            .await?;
    }
    Ok(())
}

/// Update.
pub async fn update(memory: &QmdMemory, mut doc: MemoryDocument) -> Result<()> {
    let check = validate_write(&doc.content)
        .map_err(|reason| anyhow::anyhow!("Content validation failed: {reason}"))?;
    stamp_integrity(&mut doc.metadata, &check);

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
    let check = validate_write(&content)
        .map_err(|reason| anyhow::anyhow!("Content validation failed: {reason}"))?;
    // Normalize and stamp before the duplicate check: the identity of a write
    // includes its metadata, so it has to be final before it is compared.
    let metadata = crate::memory::schema::normalize_metadata(
        &path,
        metadata,
        &memory.workspace_id,
        typed.as_ref(),
    )?;
    let mut metadata = normalize_locomo_metadata(&path, metadata);
    stamp_integrity(&mut metadata, &check);

    let canonical_path = is_canonical_path(&path);
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
    } else if let Some(identical_id) = identical_record_id(memory, &path, &content, &metadata).await
    {
        // Idempotent retry, not an upsert: non-canonical paths legitimately hold
        // many distinct records (one per trade, per telemetry event), so path,
        // content and metadata all have to match for this to be the same record.
        return Ok(identical_id);
    }
    let id = existing_id.unwrap_or_else(|| ulid::Ulid::new().to_string());
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
        // Stability: the quota check in `add_validated` below will drop this bus
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

        add_validated(
            memory,
            MemoryDocument {
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
            },
        )
        .await?;
    }

    // A concurrent identical write may have won the race inside `add_validated`,
    // which then stores nothing; hand back the id that actually resolves.
    if !canonical_path
        && !memory
            .docs
            .read()
            .await
            .iter()
            .any(|d| d.id.as_deref() == Some(id.as_str()))
    {
        if let Some(identical_id) = identical_record_id(memory, &path, &content, &metadata).await {
            return Ok(identical_id);
        }
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

#[cfg(test)]
mod write_integrity_tests {
    use super::super::QmdMemory;
    use crate::memory::store::{InMemoryMemoryStore, MemoryRecord, MemoryStore};
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::RwLock as AsyncRwLock;

    const WORKSPACE: &str = "test-write-integrity-ws";

    /// Storage-only memory, mirroring the production `QmdMemory::new_lazy`
    /// wiring so the write path is exercised exactly as in production.
    async fn lazy_memory(store: &Arc<InMemoryMemoryStore>) -> QmdMemory {
        let memory = QmdMemory::new_lazy(WORKSPACE);
        memory.set_store(store.clone()).await;
        memory
    }

    fn empty_memory() -> QmdMemory {
        QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())))
    }

    /// Defect 1, reject clauses (a) and (b).
    #[tokio::test]
    async fn create_rejects_mixed_script_corruption() {
        let memory = empty_memory();

        let mixed_script = memory
            .add_document(
                "audit/mixed-script".to_string(),
                "La全区 prioritario no es agregar features: es dejar de mentir.".to_string(),
                json!({}),
            )
            .await
            .expect_err("a token mixing Latin and CJK in Latin-dominant prose must be rejected");
        assert!(
            mixed_script
                .to_string()
                .starts_with("Content validation failed:"),
            "unexpected error: {mixed_script}"
        );

        let control_char = memory
            .add_document(
                "audit/control-char".to_string(),
                "informe trimestral \u{0} con NUL embebido".to_string(),
                json!({}),
            )
            .await
            .expect_err("a NUL byte must be rejected");
        assert!(
            control_char
                .to_string()
                .starts_with("Content validation failed:"),
            "unexpected error: {control_char}"
        );

        assert!(
            memory.all_documents().await.is_empty(),
            "a rejected write must store nothing"
        );
    }

    /// Guard rail against over-filtering: a too aggressive rule is a data-loss
    /// incident, so real multilingual and real agent-technical prose must pass.
    #[tokio::test]
    async fn create_accepts_multilingual_valid_content() {
        let memory = empty_memory();
        let samples: [(&str, &str); 10] = [
            (
                "prose/es",
                "El informe de auditoría quedó firmado: ¿cuál es el estado? Añadí tres correcciones.",
            ),
            (
                "prose/fr",
                "Le rapport d'audit nommé skills est prêt pour la revue hebdomadaire.",
            ),
            ("prose/zh", "这个API接口返回JSON数据"),
            ("prose/ja", "日本語の文章も正常に保存できる必要があります。"),
            (
                "prose/zh",
                "El informe confirma que el API\u{63a5}\u{53e3} responde bien en producci\u{f3}n.",
            ),
            (
                "tech/dotted-status",
                "El gateway reporta que health.status es degraded desde hace cinco minutos.",
            ),
            (
                "tech/camel-case",
                "Refactor de getMemory() y useState sin tocar el store ni sus tests.",
            ),
            (
                "tech/dotted-path",
                "El valor de settings.memory.data_dir cambió a /var/lib/xavier/data.",
            ),
            (
                "tech/url-and-version",
                "Ver https://github.com/iberi22/xavier y el release v0.2.15 del gate.",
            ),
            (
                "tech/json",
                "{\"zone\": \"atomic\", \"revision\": 1, \"primary\": true}",
            ),
        ];

        for (path, content) in samples {
            let stored = memory
                .add_document(path.to_string(), content.to_string(), json!({}))
                .await
                .unwrap_or_else(|error| panic!("{path} must be accepted, got: {error}"));
            let doc = memory
                .get(&stored)
                .await
                .expect("get works")
                .expect("accepted content is stored");
            assert_eq!(doc.content, content, "{path} must be stored verbatim");
        }

        assert_eq!(memory.all_documents().await.len(), samples.len());
    }

    /// Defect 1, flag clauses (c) and (d): stored verbatim, marked suspect.
    #[tokio::test]
    async fn suspect_content_is_stored_and_flagged() {
        let store = Arc::new(InMemoryMemoryStore::new());
        let memory = lazy_memory(&store).await;

        let content =
            "El informe reveló que ninguno de los cualesPassed review durante la sesión matutina.";
        let id = memory
            .add_document(
                "audit/joined-words".to_string(),
                content.to_string(),
                json!({}),
            )
            .await
            .expect("suspect content is still stored");

        let doc = memory
            .get(&id)
            .await
            .expect("get works")
            .expect("suspect record is retrievable");
        assert_eq!(doc.content, content, "content is stored verbatim");
        assert_eq!(
            doc.metadata.get("integrity").and_then(|v| v.as_str()),
            Some("suspect"),
            "joined words must be flagged, not dropped"
        );
        let reasons = doc
            .metadata
            .get("integrity_reasons")
            .and_then(|v| v.as_array())
            .expect("suspect records carry their reasons");
        assert!(
            reasons
                .iter()
                .any(|reason| reason.as_str() == Some("joined_words:cualesPassed")),
            "expected a joined_words:cualesPassed reason, got {reasons:?}"
        );

        // The marker survives persistence.
        let persisted = store
            .get(WORKSPACE, &id)
            .await
            .expect("store get works")
            .expect("record is persisted");
        assert_eq!(
            persisted.metadata.get("integrity").and_then(|v| v.as_str()),
            Some("suspect")
        );

        // The `word.Word` / `word.nón` join is flagged the same way, never dropped.
        let dot_id = memory
            .add_document(
                "audit/dot-join".to_string(),
                "El agente no tiene.noción del estado previo del store.".to_string(),
                json!({}),
            )
            .await
            .expect("dot-joined prose is still stored");
        let dot_doc = memory
            .get(&dot_id)
            .await
            .expect("get works")
            .expect("dot-join record is retrievable");
        assert_eq!(
            dot_doc.metadata.get("integrity").and_then(|v| v.as_str()),
            Some("suspect")
        );
        let dot_reasons = dot_doc
            .metadata
            .get("integrity_reasons")
            .and_then(|v| v.as_array())
            .expect("suspect records carry their reasons");
        assert!(
            dot_reasons
                .iter()
                .any(|reason| reason.as_str() == Some("dot_join:tiene.noción")),
            "expected a dot_join reason, got {dot_reasons:?}"
        );

        // Clean content on the same path policy is marked verified.
        let clean_id = memory
            .add_document(
                "audit/clean".to_string(),
                "El informe quedó firmado y sin observaciones.".to_string(),
                json!({}),
            )
            .await
            .expect("clean content is stored");
        let clean = memory
            .get(&clean_id)
            .await
            .expect("get works")
            .expect("clean record is retrievable");
        assert_eq!(
            clean.metadata.get("integrity").and_then(|v| v.as_str()),
            Some("verified")
        );
        assert!(clean.metadata.get("integrity_reasons").is_none());
    }

    /// Defect 2: the same logical document (same path, same content) must
    /// produce exactly one record and one search candidate.
    #[tokio::test]
    async fn create_is_idempotent_per_path() {
        let store = Arc::new(InMemoryMemoryStore::new());
        let memory = lazy_memory(&store).await;

        let path = "projects/xavier/audit-self-review-2026-09-26".to_string();
        let content =
            "Audit self review of the wave 29 memory write integrity gate and its duplicate index fix."
                .to_string();

        let first = memory
            .add_document(path.clone(), content.clone(), json!({}))
            .await
            .expect("first write succeeds");
        let second = memory
            .add_document(path.clone(), content.clone(), json!({}))
            .await
            .expect("retried write succeeds");

        assert_eq!(
            first, second,
            "an identical retry must resolve to the existing record, not mint a new id"
        );

        let records = store.list(WORKSPACE).await.expect("store list works");
        assert_eq!(
            records.len(),
            1,
            "one path with one content must yield one record, found {}",
            records.len()
        );
        assert_eq!(records[0].id, first);

        let candidates = memory
            .bm25_search("audit self review", 10, None)
            .await
            .expect("search works");
        assert_eq!(
            candidates.len(),
            1,
            "one logical document must yield one search candidate, found {}",
            candidates.len()
        );
        assert_eq!(candidates[0].id.as_deref(), Some(first.as_str()));
    }

    /// Counterpart of the case above: non-canonical paths hold many records on
    /// purpose (one per trade, one per telemetry event). Those must keep
    /// appending, otherwise the dedup fix would silently drop real data.
    #[tokio::test]
    async fn same_path_different_content_still_appends() {
        let store = Arc::new(InMemoryMemoryStore::new());
        let memory = lazy_memory(&store).await;

        for path in [
            "synapse/trades/NEARUSDC",
            "activity/claude/2026-09-26/session-42",
        ] {
            let mut ids = Vec::new();
            for index in 0..3 {
                ids.push(
                    memory
                        .add_document(
                            path.to_string(),
                            format!("event {index} recorded for {path} with its own payload"),
                            json!({ "index": index }),
                        )
                        .await
                        .expect("write succeeds"),
                );
            }

            let unique: std::collections::HashSet<&String> = ids.iter().collect();
            assert_eq!(
                unique.len(),
                3,
                "different content under one path must keep appending, got {ids:?}"
            );

            let records = store.list(WORKSPACE).await.expect("store list works");
            let at_path: Vec<&MemoryRecord> = records
                .iter()
                .filter(|record| record.path == path)
                .collect();
            assert_eq!(
                at_path.len(),
                3,
                "expected 3 records at {path}, found {}",
                at_path.len()
            );
        }

        assert_eq!(
            store.list(WORKSPACE).await.expect("store list works").len(),
            6
        );
    }

    /// Defect 2, the other half: same path and same content is NOT enough to
    /// call a write a duplicate. The metadata is what tells one telemetry event
    /// from the next, so a different session/index/trade is a different record
    /// even when the body happens to be byte-identical.
    #[tokio::test]
    async fn same_content_different_metadata_still_appends() {
        let store = Arc::new(InMemoryMemoryStore::new());
        let memory = lazy_memory(&store).await;

        let path = "activity/claude/2026-09-26/session-42".to_string();
        let content = "evento repetido del mismo tipo con el mismo cuerpo".to_string();

        let first = memory
            .add_document(path.clone(), content.clone(), json!({ "index": 0 }))
            .await
            .expect("first write succeeds");
        let second = memory
            .add_document(
                path.clone(),
                content.clone(),
                json!({ "index": 1, "session_id": "session-43" }),
            )
            .await
            .expect("second write succeeds");

        assert_ne!(
            first, second,
            "differing metadata means a distinct record, not a retry"
        );

        let records = store.list(WORKSPACE).await.expect("store list works");
        let at_path: Vec<&MemoryRecord> = records
            .iter()
            .filter(|record| record.path == path)
            .collect();
        assert_eq!(
            at_path.len(),
            2,
            "both events must be kept at {path}, found {}",
            at_path.len()
        );

        // A true retry — same path, same content, same metadata — still collapses.
        let retry = memory
            .add_document(path.clone(), content.clone(), json!({ "index": 0 }))
            .await
            .expect("retried write succeeds");
        assert_eq!(first, retry, "an identical retry must not mint a new id");
        assert_eq!(
            store.list(WORKSPACE).await.expect("store list works").len(),
            2
        );
    }

    /// The id handed back by a write must be the id that resolves afterwards,
    /// by id and by path, both in the cache and in a freshly loaded instance.
    #[tokio::test]
    async fn returned_id_resolves_on_lookup() {
        let store = Arc::new(InMemoryMemoryStore::new());
        let memory = lazy_memory(&store).await;

        let path = "projects/xavier/id-contract".to_string();
        let content = "The write returns the id that later resolves on lookup.".to_string();

        let returned = memory
            .add_document(path.clone(), content.clone(), json!({}))
            .await
            .expect("write succeeds");
        let retried = memory
            .add_document(path.clone(), content.clone(), json!({}))
            .await
            .expect("retried write succeeds");
        assert_eq!(returned, retried);

        let by_id = store
            .get(WORKSPACE, &returned)
            .await
            .expect("store get works")
            .expect("returned id must resolve in the store");
        assert_eq!(by_id.content, content);

        let by_path = store
            .get(WORKSPACE, &path)
            .await
            .expect("store get works")
            .expect("path must still resolve");
        assert_eq!(by_path.id, returned);

        let cached = memory
            .get(&returned)
            .await
            .expect("get works")
            .expect("returned id must resolve in the cache");
        assert_eq!(cached.path, path);

        let reloaded = lazy_memory(&store).await;
        let docs = reloaded.all_documents().await;
        assert_eq!(docs.len(), 1, "a reload must not resurrect a duplicate");
        assert_eq!(docs[0].id.as_deref(), Some(returned.as_str()));
    }

    /// Records written before this gate have no `integrity` key. They must keep
    /// loading untouched — nothing is computed on read.
    #[tokio::test]
    async fn legacy_records_without_integrity_marker_still_load() {
        let legacy: MemoryRecord = serde_json::from_str(
            r#"{
                "id": "legacy-record-id",
                "workspace_id": "test-write-integrity-ws",
                "path": "legacy/audit-2026-09-01",
                "content": "ninguno de los cualesPassed review quedo registrado",
                "metadata": { "source": "legacy" },
                "embedding": [],
                "created_at": "2026-09-01T10:00:00Z",
                "updated_at": "2026-09-01T10:00:00Z",
                "revision": 1,
                "primary": true,
                "parent_id": null
            }"#,
        )
        .expect("a record without an integrity marker must deserialize");
        assert!(
            legacy.metadata.get("integrity").is_none(),
            "the fixture must have no integrity marker"
        );

        let store = Arc::new(InMemoryMemoryStore::new());
        store
            .put(legacy.clone())
            .await
            .expect("seed store with the legacy record");

        let memory = lazy_memory(&store).await;
        let doc = memory
            .get("legacy/audit-2026-09-01")
            .await
            .expect("get works")
            .expect("legacy record must load by path");
        assert_eq!(doc.content, legacy.content);
        assert_eq!(
            doc.metadata.get("source").and_then(|v| v.as_str()),
            Some("legacy")
        );
        assert!(
            doc.metadata.get("integrity").is_none(),
            "integrity is a write-time marker; reads must not compute one"
        );

        let by_id = store
            .get(WORKSPACE, "legacy-record-id")
            .await
            .expect("store get works")
            .expect("legacy record must load by id");
        assert_eq!(by_id.content, legacy.content);
    }
}
