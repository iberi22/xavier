//! API handlers for memory and workspace operations.
//!
//! This module provides endpoints for multi-layer memory retrieval, context export,
//! memory curation, and automated background maintenance tasks like decay and consolidation.

use crate::consistency::regularization::RetentionRegularizer;
use crate::consolidation::ConsolidationTask;
use crate::context::ContextClassifier;
use crate::retrieval::gating::{AdaptiveGating, LayerWeights, SessionSummary};
use crate::server::http::types::*;
use crate::workspace::WorkspaceContext;
use axum::{extract::Json, response::IntoResponse, Extension};
use std::sync::Arc;

pub async fn memory_retrieve(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<MultiLayerRetrieveRequest>,
) -> impl IntoResponse {
    let settings = crate::settings::XavierSettings::current();
    let active_zones = payload
        .active_zones
        .clone()
        .unwrap_or_else(|| crate::memory::schema::parse_zones_from_prompt(&payload.query));
    let weights = if let Some(w) = payload.layer_weights {
        w
    } else {
        let classifier = ContextClassifier::new();
        let level = classifier.classify(&payload.query);
        LayerWeights::adaptive(&payload.query, level, &[])
    };
    let gating_config = crate::retrieval::gating::GatingConfig {
        layer_weights: weights,
        relevance_threshold: payload.relevance_threshold.clamp(0.0, 1.0),
        rrf_k: payload.rrf_k,
        max_results: payload.limit.max(1),
        active_zones: Some(active_zones),
        zone_boost_multiplier: settings
            .retrieval
            .zone_boost_multiplier
            .unwrap_or_else(crate::retrieval::config::configured_zone_boost),
        zone_penalty_multiplier: settings
            .retrieval
            .zone_penalty_multiplier
            .unwrap_or_else(crate::retrieval::config::configured_zone_penalty),
        recency_weight: payload.recency_weight,
        half_life_hours: payload.half_life_hours,
        grounding_enabled: payload.grounding_enabled,
        grounding_min_confidence: payload.grounding_min_confidence,
        navigation_policy: Some(crate::retrieval::NavigationPolicy::default()),
        cache_warming_enabled: settings.retrieval.cache_warming_enabled,
        cache_warming_threshold: settings
            .retrieval
            .cache_warming_threshold
            .unwrap_or(crate::retrieval::config::DEFAULT_CACHE_WARMING_THRESHOLD),
    };

    let mut gating = if payload.layer_weights.is_some() {
        AdaptiveGating::new(gating_config)
    } else {
        AdaptiveGating::with_policy(gating_config, workspace.workspace.hormer.policy().clone())
    };
    gating = gating
        .with_memory(Arc::clone(&workspace.workspace.memory))
        .with_booster(Arc::clone(&workspace.workspace.zone_booster));

    let working_docs = workspace.workspace.working_documents().await;

    let threads = workspace
        .workspace
        .conversations_db
        .list_threads(50)
        .await
        .unwrap_or_default();
    let mut episodic_summaries = Vec::new();
    for s in threads {
        let summary = if let Some(ref preview) = s.last_preview {
            if preview.contains("### Extractive Session Summary") {
                preview.clone()
            } else {
                if let Ok(messages) = workspace.workspace.conversations_db.get_thread_messages(&s.id).await {
                    if !messages.is_empty() {
                        let gen = crate::memory::episodic::summarize_session_extractive(&messages);
                        let _ = workspace.workspace.conversations_db.update_last_preview(&s.id, &gen).await;
                        gen
                    } else {
                        preview.clone()
                    }
                } else {
                    preview.clone()
                }
            }
        } else {
            if let Ok(messages) = workspace.workspace.conversations_db.get_thread_messages(&s.id).await {
                if !messages.is_empty() {
                    let gen = crate::memory::episodic::summarize_session_extractive(&messages);
                    let _ = workspace.workspace.conversations_db.update_last_preview(&s.id, &gen).await;
                    gen
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };
        episodic_summaries.push(SessionSummary {
            session_id: s.id.clone(),
            start_time: s.started_at,
            summary,
            key_events: vec![],
            sentiment_timeline: vec![],
        });
    }
    let semantic_entities = workspace.workspace.entity_graph.all_entities().await;
    let results = gating
        .retrieve_with_telemetry(
            &working_docs,
            &episodic_summaries,
            &semantic_entities,
            &payload.query,
            Some(std::sync::Arc::clone(&workspace.workspace.belief_graph)),
            None,
            Some(workspace.workspace_id.clone()), // Use workspace_id as user_id for session adaptation
        )
        .await;

    // HORMER: Update policy if it was used
    if payload.layer_weights.is_none() {
        let weights_used = gating.effective_weights().await;
        workspace
            .workspace
            .hormer
            .update_from_interaction(weights_used, &results, Some(&workspace.workspace_id))
            .await;
    } else {
        workspace.workspace.hormer.record_non_navigated();
    }

    let retrieved: Vec<RetrievedMemory> = results
        .iter()
        .map(|r| RetrievedMemory {
            path: retrieved_path_for_result(&working_docs, &r.id, &r.source),
            id: r.id.clone(),
            content: r.content.clone(),
            score: r.score,
            source_layer: r.source.clone(),
        })
        .collect();
    let coherence_report = if payload.include_coherence {
        let regularizer = RetentionRegularizer::with_defaults();
        Some(regularizer.check_coherence_with_entities(&working_docs, &semantic_entities))
    } else {
        None
    };
    Json(MultiLayerRetrieveResponse {
        status: "ok".to_string(),
        results: retrieved,
        query: payload.query.clone(),
        layers_used: LayerStatsJson {
            working_count: working_docs.len(),
            episodic_count: episodic_summaries.len(),
            semantic_count: semantic_entities.len(),
            total_results: results.len(),
        },
        coherence_report,
    })
}

fn retrieved_path_for_result(
    working_docs: &[crate::memory::qmd_memory::MemoryDocument],
    result_id: &str,
    source_layer: &str,
) -> String {
    if let Some(path) = working_docs
        .iter()
        .find(|document| document.id.as_deref() == Some(result_id))
        .map(|document| document.path.clone())
    {
        return path;
    }
    match source_layer {
        "episodic" => format!("panel/threads/{result_id}"),
        "semantic" => format!("semantic/entities/{result_id}"),
        _ => result_id.to_string(),
    }
}

pub async fn memory_export_pack(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<ExportPackRequest>,
) -> impl IntoResponse {
    let gating = AdaptiveGating::with_defaults();

    let working_docs = workspace.workspace.working_documents().await;

    let belief_filters = crate::memory::schema::MemoryQueryFilters {
        levels: Some(vec![crate::memory::schema::MemoryLevel::Belief]),
        ..Default::default()
    };
    let belief_records = workspace
        .workspace
        .list_memory_records_filtered(belief_filters, 1000)
        .await
        .unwrap_or_default();
    let all_docs: Vec<_> = belief_records
        .into_iter()
        .map(|r| r.to_document())
        .collect();

    let threads = workspace
        .workspace
        .conversations_db
        .list_threads(50)
        .await
        .unwrap_or_default();
    let mut episodic_summaries = Vec::new();
    for s in threads {
        let summary = if let Some(ref preview) = s.last_preview {
            if preview.contains("### Extractive Session Summary") {
                preview.clone()
            } else {
                if let Ok(messages) = workspace.workspace.conversations_db.get_thread_messages(&s.id).await {
                    if !messages.is_empty() {
                        let gen = crate::memory::episodic::summarize_session_extractive(&messages);
                        let _ = workspace.workspace.conversations_db.update_last_preview(&s.id, &gen).await;
                        gen
                    } else {
                        preview.clone()
                    }
                } else {
                    preview.clone()
                }
            }
        } else {
            if let Ok(messages) = workspace.workspace.conversations_db.get_thread_messages(&s.id).await {
                if !messages.is_empty() {
                    let gen = crate::memory::episodic::summarize_session_extractive(&messages);
                    let _ = workspace.workspace.conversations_db.update_last_preview(&s.id, &gen).await;
                    gen
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };
        episodic_summaries.push(SessionSummary {
            session_id: s.id.clone(),
            start_time: s.started_at,
            summary,
            key_events: vec![],
            sentiment_timeline: vec![],
        });
    }
    let semantic_entities = workspace.workspace.entity_graph.all_entities().await;
    let layered_result = gating
        .retrieve_layered(
            &working_docs,
            &all_docs,
            &episodic_summaries,
            &semantic_entities,
            &payload.topic,
        )
        .await;
    let xml = crate::memory::pack::generate_xcp(layered_result, payload.max_level);
    Json(ExportPackResponse {
        status: "ok".to_string(),
        xml,
        filename: format!("context-{}.xcp", payload.topic.replace(" ", "_")),
    })
}

pub async fn memory_curate(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Some(id) = payload.get("id").and_then(|id| id.as_str()) {
        let action = crate::memory::manager::MemoryAction::Curate {
            doc_id: id.to_string(),
        };
        match workspace
            .workspace
            .memory_manager
            .execute_actions(vec![action])
            .await
        {
            Ok(_) => {
                let _ = workspace.workspace.persist_beliefs().await;
                Json(serde_json::json!({ "status": "ok", "message": "Curation completed" }))
            }
            Err(e) => Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
        }
    } else {
        Json(serde_json::json!({ "status": "error", "message": "Missing 'id' in request body" }))
    }
}

pub async fn memory_manage(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    match workspace.workspace.memory_manager.auto_manage().await {
        Ok(count) => {
            let _ = workspace.workspace.persist_beliefs().await;
            Json(serde_json::json!({ "status": "ok", "actions_executed": count }))
        }
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
    }
}

pub async fn memory_decay(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    match workspace.workspace.memory_manager.decay_memories().await {
        Ok(result) => {
            let _ = workspace.workspace.persist_beliefs().await;
            Json(
                serde_json::json!({ "status": "ok", "documents_affected": result.documents_affected, "actions": result.actions.len(), "bytes_freed": result.bytes_freed }),
            )
        }
        Err(e) => Json(serde_json::json!({"status": "error", "message": e.to_string() })),
    }
}

pub async fn memory_consolidate(
    Extension(workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    let task = ConsolidationTask::default();
    match task.consolidate(&workspace, None).await {
        Ok(stats) => Json(serde_json::json!({ "status": "ok", "stats": stats })),
        Err(e) => Json(serde_json::json!({"status": "error", "message": e.to_string() })),
    }
}

pub async fn memory_reflect(
    Extension(workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    let task = ConsolidationTask::default();
    match task.reflect(&workspace).await {
        Ok(result) => Json(serde_json::json!({ "status": "ok", "data": result })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e.to_string() })),
    }
}
