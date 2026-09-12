//! Code handlers for scanning, searching, and analyzing codebases.

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

use crate::cli::code_dump::{perform_dump, perform_load};
use crate::cli::security::secure_optional_request_field;
use crate::cli::state::CliState;
use crate::cli::types::*;
use crate::cli::utils::estimate_tokens;
use code_graph::types::{CodeEdge, Symbol, SymbolKind};
use xavier::codebase::codegraph_paths::code_graph_db_path_for;
use xavier::codebase::codegraph_sidecar::{
    ensure_codegraph_sidecar, ensure_codegraph_sidecar_soft, maybe_sync_colby_project,
    EnsureOptions, EnsureOutcome, InstallMode,
};
use xavier::codebase::connection_manager::ConnectionManager;

use xavier::ports::inbound::input_security_port::SecureInputResult;

fn ensure_sidecar_for_workspace(workspace: &std::path::Path) -> EnsureOutcome {
    ensure_codegraph_sidecar_soft(workspace)
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct InstallSidecarRequest {
    #[serde(default)]
    pub reprompt: Option<bool>,
    #[serde(default)]
    pub force: Option<bool>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// Code sidecar status handler (`GET /code/sidecar/status`).
pub async fn code_sidecar_status_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let outcome = ensure_codegraph_sidecar_soft(&state.workspace_dir);
    let path = outcome.bin_path.map(|p| p.to_string_lossy().to_string());

    axum::Json(serde_json::json!({
        "available": outcome.available,
        "path": path,
        "version": Option::<String>::None,
        "engine": "colby",
        "message": outcome.message,
    }))
}

/// Code sidecar install handler (`POST /code/install`).
pub async fn code_sidecar_install_handler(
    State(state): State<CliState>,
    payload: Option<Json<InstallSidecarRequest>>,
) -> impl axum::response::IntoResponse {
    let req = payload.map(|Json(p)| p).unwrap_or_default();
    let reprompt = req.reprompt.unwrap_or(false);
    let install_mode = match req.mode.as_deref() {
        Some("ask") => InstallMode::Ask,
        _ => InstallMode::No,
    };

    let opts = EnsureOptions {
        reprompt,
        install_mode,
    };

    let outcome = ensure_codegraph_sidecar(&state.workspace_dir, opts);
    let path = outcome.bin_path.map(|p| p.to_string_lossy().to_string());

    axum::Json(serde_json::json!({
        "status": if outcome.available { "ok" } else { "unavailable" },
        "available": outcome.available,
        "path": path,
        "version": Option::<String>::None,
        "engine": "colby",
        "message": outcome.message,
    }))
}

// ── XAV-01: explicit per-repo graph resolution ──────────────────────────
// The CLI sends the cwd-derived repo identity (project_id + root +
// indexed_commit) on stats/find. The server anchors the graph at that root
// (`<root>/.xavier/code_graph.db`) and NEVER falls back to another repo's
// graph: a missing/empty/stale index is reported as degraded FOR THAT repo.

/// Server-side resolution of a requested repo.
struct ResolvedRepo {
    project_id: String,
    root: PathBuf,
    db_path: PathBuf,
    indexed_commit: String,
    head: String,
    stale: bool,
    project_id_mismatch: bool,
}

/// Resolve the caller's requested repo to its on-disk graph location.
///
/// Returns `None` when no usable `root` was supplied (legacy callers) or
/// the supplied root is not an existing directory. Callers must report a
/// missing repo as degraded for THAT repo, never another repo's graph.
fn resolve_requested_repo(project_id: Option<&str>, root: Option<&str>) -> Option<ResolvedRepo> {
    let raw = root.filter(|s| !s.trim().is_empty())?;
    let abs = std::path::absolute(raw).ok()?;
    if !abs.is_dir() {
        return None;
    }
    let repo_root = xavier::codebase::repo_identity::find_repo_root(&abs);
    let canonical = repo_root.canonicalize().unwrap_or(repo_root);
    let derived = xavier::codebase::repo_identity::derive_project_id(&canonical);
    let supplied = project_id.filter(|s| !s.trim().is_empty());
    if let Some(pid) = supplied {
        if xavier::codebase::validate_project_id(pid).is_err() {
            return None;
        }
    }
    let project_id_mismatch = supplied.is_some_and(|pid| pid != derived);
    let indexed_commit = xavier::codebase::repo_identity::read_indexed_commit(&canonical);
    let head = xavier::codebase::repo_identity::git_head(&canonical)
        .unwrap_or_else(|| "unknown".to_string());
    let stale = indexed_commit != "unknown" && head != "unknown" && indexed_commit != head;
    Some(ResolvedRepo {
        project_id: derived,
        root: canonical.clone(),
        db_path: xavier::codebase::repo_identity::code_graph_db_path_for_root(&canonical),
        indexed_commit,
        head,
        stale,
        project_id_mismatch,
    })
}

/// Canonical repo root of the daemon's own startup workspace.
///
/// Used ONLY to decide whether the legacy global graph belongs to the
/// requested repo (same-repo migration fallback). It never authorizes
/// serving one repo's graph for a different repo.
fn workspace_repo_root(workspace_dir: &std::path::Path) -> PathBuf {
    let root = xavier::codebase::repo_identity::find_repo_root(workspace_dir);
    root.canonicalize().unwrap_or(root)
}

/// True when the requested repo IS the daemon's workspace repo, in which
/// case the legacy global graph (`state.code_graph`) is that repo's own
/// pre-XAV-01 index and may serve as a same-repo fallback while no
/// per-repo `<root>/.xavier/code_graph.db` exists yet.
fn is_workspace_repo(workspace_dir: &std::path::Path, requested: &ResolvedRepo) -> bool {
    workspace_repo_root(workspace_dir) == requested.root
}

/// Shared `repo` echo object for per-repo responses.
fn repo_json(r: &ResolvedRepo) -> serde_json::Value {
    serde_json::json!({
        "project_id": r.project_id,
        "root": r.root.to_string_lossy(),
        "indexed_commit": r.indexed_commit,
        "head": r.head,
        "stale": r.stale,
    })
}

/// Degraded stats for a repo whose index is missing/empty/stale.
/// Totals are zeroed only when there is no data; identity always echoed.
fn degraded_stats_response(
    project_id: &str,
    root: &str,
    indexed_commit: &str,
    reason: &str,
    detail: String,
) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "total_files": 0,
        "total_symbols": 0,
        "total_imports": 0,
        "languages": [],
        "degraded": true,
        "degraded_reason": reason,
        "warning": detail,
        "project_id": project_id,
        "root": root,
        "indexed_commit": indexed_commit,
    }))
}

/// Legacy global stats (pre-XAV-01 single-workspace graph).
async fn legacy_stats_json(state: &CliState) -> serde_json::Value {
    let db_path = code_graph_db_path_for(&state.workspace_dir);
    let db_stats = if let Ok(db) = ConnectionManager::global().get_code_graph_db(&db_path) {
        db.stats()
    } else {
        let code_graph = state.code_graph.read().await;
        code_graph.db.stats()
    };

    match db_stats {
        Ok(stats) => {
            let empty = stats.total_symbols == 0;
            serde_json::json!({
                "status": "ok",
                "total_files": stats.total_files,
                "total_symbols": stats.total_symbols,
                "total_imports": stats.total_imports,
                "languages": stats.languages,
                "degraded": empty,
                "warning": if empty {
                    Some("CodeGraph vacío (total_symbols=0). Ejecuta `xavier code scan .` o `xavier code sync --git`.")
                } else {
                    None
                },
            })
        }
        Err(error) => serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "total_files": 0,
            "total_symbols": 0,
            "total_imports": 0,
            "degraded": true,
        }),
    }
}

/// Inject the resolved repo identity into a stats body built from the
/// same-repo legacy global index.
fn with_repo_echo(
    mut body: serde_json::Value,
    r: &ResolvedRepo,
    legacy: bool,
) -> serde_json::Value {
    body["repo"] = repo_json(r);
    body["project_id"] = serde_json::json!(r.project_id);
    body["root"] = serde_json::json!(r.root.to_string_lossy());
    body["indexed_commit"] = serde_json::json!(r.indexed_commit);
    if legacy {
        body["legacy_index"] = serde_json::json!(true);
    }
    if r.stale && body.get("status") == Some(&serde_json::json!("ok")) {
        body["degraded"] = serde_json::json!(true);
        body["degraded_reason"] = serde_json::json!("stale");
        body["warning"] = serde_json::json!(format!(
            "Índice desactualizado para '{}': indexado={} HEAD={}. Ejecuta `xavier code sync --git`.",
            r.root.display(),
            r.indexed_commit,
            r.head
        ));
    } else if r.project_id_mismatch && body.get("warning").is_none_or(|w| w.is_null()) {
        body["warning"] = serde_json::json!(format!(
            "project_id solicitado difiere del derivado para '{}'.",
            r.root.display()
        ));
    }
    body
}

/// Auto-index `src/` directory into the code graph.
///
/// Scans all source files under the project's `src/` directory and builds
/// the code graph with symbols, imports, and relationships.
pub async fn code_index_handler(
    State(state): State<CliState>,
    payload: Option<axum::Json<serde_json::Value>>,
) -> Json<serde_json::Value> {
    let base_path = payload
        .as_ref()
        .and_then(|payload| payload.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("src");
    info!("Code index request: path={}", base_path);

    let sidecar = ensure_sidecar_for_workspace(&state.workspace_dir);

    let code_graph = state.code_graph.read().await;
    match code_graph
        .indexer
        .index(std::path::Path::new(base_path), true)
        .await
    {
        Ok(stats) => {
            if let Some(bin) = sidecar.bin_path.as_ref() {
                maybe_sync_colby_project(std::path::Path::new(base_path), bin);
            }

            // Automatically trigger soft dump after successful index
            let (dump_msg, dump_path_str, dump_success) =
                match perform_dump(&code_graph, base_path).await {
                    Ok(dump_path) => (
                        format!(" (Dumped to {})", dump_path.display()),
                        Some(dump_path.to_string_lossy().into_owned()),
                        true,
                    ),
                    Err(e) => (format!(" (Dump failed: {})", e), None, false),
                };

            Json(serde_json::json!({
                "status": "ok",
                "indexed_files": stats.total_files,
                "indexed_symbols": stats.total_symbols,
                "indexed_imports": stats.total_imports,
                "duration_ms": stats.duration_ms,
                "paths": [base_path],
                "languages": stats.languages,
                "codegraph_sidecar": sidecar.message,
                "codegraph_available": sidecar.available,
                "codegraph_dump_path": dump_path_str,
                "codegraph_dump_success": dump_success,
                "message": format!("Indexed {} files, {} symbols, {} imports across {:?}{}",
                    stats.total_files, stats.total_symbols, stats.total_imports, stats.languages, dump_msg),
            }))
        }
        Err(error) => Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "indexed_files": 0,
            "indexed_symbols": 0,
            "indexed_imports": 0,
            "paths": [base_path],
            "codegraph_sidecar": sidecar.message,
            "codegraph_available": sidecar.available,
        })),
    }
}

/// Code memories handler (`POST /code/memories`).
/// Returns agent memories linked to a specific code symbol.
pub async fn code_memories_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeMemoriesPayload>,
) -> impl axum::response::IntoResponse {
    let limit = payload.limit.clamp(1, 100);

    let sec_result = state
        .security
        .process_input(&payload.symbol)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.symbol.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let symbol = sec_result
        .sanitized_input
        .unwrap_or_else(|| sec_result.original_input.clone());

    let code_graph = state.code_graph.read().await;
    let links = code_graph
        .query
        .memories_for_symbol_limit(&symbol, limit)
        .unwrap_or_default();

    let mut memories = Vec::new();
    for link in &links {
        if let Ok(Some(rec)) = state.store.get(&state.workspace_id, &link.memory_id).await {
            memories.push(serde_json::json!({
                "memory_id": rec.id,
                "path": rec.path,
                "content": rec.content,
                "confidence": link.confidence,
                "created_at": rec.created_at,
            }));
        } else {
            memories.push(serde_json::json!({
                "memory_id": link.memory_id,
                "symbol_id": link.symbol_id,
                "confidence": link.confidence,
            }));
        }
    }

    axum::Json(serde_json::json!({
        "status": "ok",
        "symbol": symbol,
        "count": memories.len(),
        "memories": memories,
    }))
}

/// Code dump handler.
pub async fn code_dump_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    let path = payload.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let code_graph = state.code_graph.read().await;
    match perform_dump(&code_graph, path).await {
        Ok(dump_path) => axum::Json(serde_json::json!({
            "status": "ok",
            "message": format!("Code graph dumped to {}", dump_path.display()),
            "path": dump_path.to_string_lossy(),
        })),
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

/// Code load handler.
pub async fn code_load_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    let path = payload.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    match perform_load(path).await {
        Ok(new_state) => {
            let mut code_graph = state.code_graph.write().await;
            *code_graph = new_state;
            axum::Json(serde_json::json!({
                "status": "ok",
                "message": "Code graph loaded successfully from dump",
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

/// Code scan handler.
pub async fn code_scan_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeScanPayload>,
) -> impl axum::response::IntoResponse {
    let requested_path = payload.path.unwrap_or_else(|| ".".to_string());

    let sec_result = state
        .security
        .process_input(&requested_path)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: requested_path.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "code/scan blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let workspace_root =
        std::path::absolute(&state.workspace_dir).unwrap_or_else(|_| PathBuf::from("."));
    let Ok(abs_path) = std::path::absolute(&requested_path) else {
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "invalid path",
            "indexed_files": 0,
        }));
    };
    if !abs_path.starts_with(&workspace_root) {
        warn!(
            "Path traversal blocked: {} is outside workspace root {}",
            abs_path.display(),
            workspace_root.display()
        );
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "path outside workspace not allowed",
            "indexed_files": 0,
        }));
    }

    let path = requested_path;
    info!("Code scan request: path={}", path);

    // Consent-first Colby sidecar (server usually non-TTY → skip/honour env). Soft-fail.
    let sidecar = ensure_sidecar_for_workspace(&state.workspace_dir);

    let code_graph = state.code_graph.read().await;
    match code_graph
        .indexer
        .index(std::path::Path::new(&path), true)
        .await
    {
        Ok(stats) => {
            if let Some(bin) = sidecar.bin_path.as_ref() {
                maybe_sync_colby_project(std::path::Path::new(&path), bin);
            }

            // Automatically trigger soft dump after successful scan
            let (dump_msg, dump_path_str, dump_success) =
                match perform_dump(&code_graph, &path).await {
                    Ok(dump_path) => (
                        format!(" (Dumped to {})", dump_path.display()),
                        Some(dump_path.to_string_lossy().into_owned()),
                        true,
                    ),
                    Err(e) => (format!(" (Dump failed: {})", e), None, false),
                };

            axum::Json(serde_json::json!({
                "status": "ok",
                "indexed_files": stats.total_files,
                "indexed_symbols": stats.total_symbols,
                "indexed_imports": stats.total_imports,
                "duration_ms": stats.duration_ms,
                "paths": [path],
                "languages": stats.languages,
                "codegraph_sidecar": sidecar.message,
                "codegraph_available": sidecar.available,
                "codegraph_dump_path": dump_path_str,
                "codegraph_dump_success": dump_success,
                "message": format!("Scan complete. {}{}", stats.to_string(), dump_msg),
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "indexed_files": 0,
            "indexed_symbols": 0,
            "indexed_imports": 0,
            "paths": [path],
            "codegraph_sidecar": sidecar.message,
            "codegraph_available": sidecar.available,
        })),
    }
}

/// Code find handler.
pub async fn code_find_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeFindPayload>,
) -> impl axum::response::IntoResponse {
    let query_to_process = if payload.query.is_empty() {
        payload.name.as_deref().unwrap_or("").to_string()
    } else {
        payload.query.clone()
    };

    let sec_result = state
        .security
        .process_input(&query_to_process)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: query_to_process.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "code/find blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let query = sec_result
        .sanitized_input
        .as_deref()
        .unwrap_or(&sec_result.original_input)
        .to_string();

    let name = match secure_optional_request_field(
        state.security.as_ref(),
        "code/find name",
        payload.name.as_deref(),
    )
    .await
    {
        Ok(name) => name,
        Err(sec_result) => {
            info!(
                "code/find blocked by security: name rejected (confidence={})",
                sec_result.detection_confidence
            );
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "name",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };

    let pattern = match secure_optional_request_field(
        state.security.as_ref(),
        "code/find pattern",
        payload.pattern.as_deref(),
    )
    .await
    {
        Ok(pattern) => pattern,
        Err(sec_result) => {
            info!(
                "code/find blocked by security: pattern rejected (confidence={})",
                sec_result.detection_confidence
            );
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "pattern",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };
    let kind = match secure_optional_request_field(
        state.security.as_ref(),
        "code/find kind",
        payload.kind.as_deref(),
    )
    .await
    {
        Ok(kind) => kind,
        Err(sec_result) => {
            info!(
                "code/find blocked by security: kind rejected (confidence={})",
                sec_result.detection_confidence
            );
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "kind",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };
    let limit = payload.limit.clamp(1, 100);
    info!(
        "Code find request: query={}, limit={}, kind={:?}, pattern={:?}, name={:?}",
        query, limit, kind, pattern, name
    );

    // XAV-01: explicit repo → resolve THAT repo's graph, never the daemon's
    // startup workspace. Missing/empty index → degraded for that repo.
    if let Some(repo) = payload.repo.as_ref() {
        let repo_root = repo.root.clone().unwrap_or_default();
        if repo_root.trim().is_empty() {
            return axum::Json(serde_json::json!({
                "status": "error",
                "message": "repo.root vacío: el CLI debe enviar la identidad del repo (XAV-01)",
                "query": query,
                "count": 0,
                "results": [],
                "degraded": true,
            }));
        }
        let Some(r) = resolve_requested_repo(repo.project_id.as_deref(), repo.root.as_deref())
        else {
            return axum::Json(serde_json::json!({
                "status": "ok",
                "query": query,
                "count": 0,
                "results": [],
                "degraded": true,
                "degraded_reason": "missing",
                "warning": format!("No hay índice CodeGraph para '{}'. Ejecuta `xavier code scan .` en ese repo.", repo_root),
                "project_id": repo.project_id.clone().unwrap_or_else(|| "unknown".to_string()),
                "root": repo_root,
                "indexed_commit": repo.indexed_commit.clone().unwrap_or_else(|| "unknown".to_string()),
            }));
        };
        if !r.db_path.exists() {
            // Same-repo migration: the daemon's pre-XAV-01 global graph IS
            // this repo's index. Any other repo gets degraded — never the
            // workspace's graph.
            if is_workspace_repo(&state.workspace_dir, &r) {
                let legacy_db_path = code_graph_db_path_for(&state.workspace_dir);
                let legacy_db = match ConnectionManager::global().get_code_graph_db(&legacy_db_path)
                {
                    Ok(db) => db,
                    Err(error) => {
                        return axum::Json(serde_json::json!({
                            "status": "error",
                            "message": format!("No se pudo abrir el índice de '{}': {}", r.root.display(), error),
                            "query": query,
                            "count": 0,
                            "results": [],
                            "degraded": true,
                            "legacy_index": true,
                            "repo": repo_json(&r),
                            "project_id": r.project_id,
                            "root": r.root.to_string_lossy(),
                            "indexed_commit": r.indexed_commit,
                        }));
                    }
                };
                if legacy_db.stats().map(|s| s.total_symbols).unwrap_or(0) == 0 {
                    return axum::Json(serde_json::json!({
                        "status": "ok",
                        "query": query,
                        "count": 0,
                        "results": [],
                        "degraded": true,
                        "degraded_reason": "empty",
                        "legacy_index": true,
                        "warning": format!("CodeGraph vacío para '{}' (total_symbols=0). Ejecuta `xavier code scan .` en ese repo.", r.root.display()),
                        "repo": repo_json(&r),
                        "project_id": r.project_id,
                        "root": r.root.to_string_lossy(),
                        "indexed_commit": r.indexed_commit,
                    }));
                }
                let legacy_query = code_graph::query::QueryEngine::new(Arc::new(legacy_db));
                let symbols = code_find_symbols(
                    &legacy_query,
                    &query,
                    name.as_deref(),
                    kind.as_deref(),
                    pattern.as_deref(),
                    limit,
                );
                let results = find_results_json(symbols);
                let mut body = serde_json::json!({
                    "status": "ok",
                    "query": query,
                    "count": results.len(),
                    "results": results,
                    "degraded": false,
                    "legacy_index": true,
                    "repo": repo_json(&r),
                    "project_id": r.project_id,
                    "root": r.root.to_string_lossy(),
                    "indexed_commit": r.indexed_commit,
                });
                if r.stale {
                    body["degraded"] = serde_json::json!(true);
                    body["degraded_reason"] = serde_json::json!("stale");
                    body["warning"] = serde_json::json!(format!(
                        "Índice desactualizado para '{}': indexado={} HEAD={}. Ejecuta `xavier code sync --git`.",
                        r.root.display(),
                        r.indexed_commit,
                        r.head
                    ));
                }
                return axum::Json(body);
            }
            return axum::Json(serde_json::json!({
                "status": "ok",
                "query": query,
                "count": 0,
                "results": [],
                "degraded": true,
                "degraded_reason": "missing",
                "warning": format!("No hay índice CodeGraph para '{}' (falta {}). Ejecuta `xavier code scan .` en ese repo.", r.root.display(), r.db_path.display()),
                "repo": repo_json(&r),
                "project_id": r.project_id,
                "root": r.root.to_string_lossy(),
                "indexed_commit": r.indexed_commit,
            }));
        }
        let repo_db = match ConnectionManager::global().get_code_graph_db(&r.db_path) {
            Ok(db) => db,
            Err(error) => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": format!("No se pudo abrir el índice de '{}': {}", r.root.display(), error),
                    "query": query,
                    "count": 0,
                    "results": [],
                    "degraded": true,
                    "repo": repo_json(&r),
                    "project_id": r.project_id,
                    "root": r.root.to_string_lossy(),
                    "indexed_commit": r.indexed_commit,
                }));
            }
        };
        if repo_db.stats().map(|s| s.total_symbols).unwrap_or(0) == 0 {
            return axum::Json(serde_json::json!({
                "status": "ok",
                "query": query,
                "count": 0,
                "results": [],
                "degraded": true,
                "degraded_reason": "empty",
                "warning": format!("CodeGraph vacío para '{}' (total_symbols=0). Ejecuta `xavier code scan .` en ese repo.", r.root.display()),
                "repo": repo_json(&r),
                "project_id": r.project_id,
                "root": r.root.to_string_lossy(),
                "indexed_commit": r.indexed_commit,
            }));
        }
        let repo_query = code_graph::query::QueryEngine::new(Arc::new(repo_db));
        let symbols = code_find_symbols(
            &repo_query,
            &query,
            name.as_deref(),
            kind.as_deref(),
            pattern.as_deref(),
            limit,
        );
        let results = find_results_json(symbols);
        let mut body = serde_json::json!({
            "status": "ok",
            "query": query,
            "count": results.len(),
            "results": results,
            "degraded": r.stale,
            "repo": repo_json(&r),
            "project_id": r.project_id,
            "root": r.root.to_string_lossy(),
            "indexed_commit": r.indexed_commit,
        });
        if r.stale {
            body["degraded_reason"] = serde_json::json!("stale");
            body["warning"] = serde_json::json!(format!(
                "Índice desactualizado para '{}': indexado={} HEAD={}. Ejecuta `xavier code sync --git`.",
                r.root.display(),
                r.indexed_commit,
                r.head
            ));
        }
        return axum::Json(body);
    }

    let code_graph = state.code_graph.read().await;
    let symbols = code_find_symbols(
        &code_graph.query,
        &query,
        name.as_deref(),
        kind.as_deref(),
        pattern.as_deref(),
        limit,
    );

    let results = find_results_json(symbols);

    axum::Json(serde_json::json!({
        "status": "ok",
        "query": query,
        "count": results.len(),
        "results": results,
    }))
}

/// Render find results as JSON values (shared by per-repo and legacy paths).
fn find_results_json(symbols: Vec<code_graph::types::Symbol>) -> Vec<serde_json::Value> {
    symbols
        .into_iter()
        .map(|symbol| {
            serde_json::json!({
                "id": symbol.id,
                "stable_id": symbol.stable_id,
                "path": symbol.file_path,
                "symbol": symbol.name,
                "symbol_type": format!("{:?}", symbol.kind),
                "language": format!("{:?}", symbol.lang),
                "line": symbol.start_line,
                "end_line": symbol.end_line,
                "signature": symbol.signature,
                "parent": symbol.parent,
                "complexity": symbol.complexity,
            })
        })
        .collect()
}

/// Bridge between Xavier's Embedder and code_graph's SymbolEmbedder
pub struct XavierSymbolEmbedder {
    embedder: std::sync::Arc<dyn xavier::embedding::Embedder>,
}

impl XavierSymbolEmbedder {
    pub fn new(embedder: std::sync::Arc<dyn xavier::embedding::Embedder>) -> Self {
        Self { embedder }
    }
}

#[async_trait::async_trait]
impl code_graph::types::SymbolEmbedder for XavierSymbolEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, code_graph::GraphError> {
        self.embedder
            .encode(text)
            .await
            .map_err(|e| code_graph::GraphError::Database(e.to_string()))
    }
}

/// Code search handler (BM25, semantic, or hybrid mode).
pub async fn code_search_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeSearchPayload>,
) -> impl axum::response::IntoResponse {
    let sec_result = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "code/search blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let query = sec_result
        .sanitized_input
        .as_deref()
        .unwrap_or(&sec_result.original_input)
        .to_string();

    let limit = payload.limit.clamp(1, 100);
    let mode = payload.mode.to_lowercase();
    info!(
        "Code search request: query={}, mode={}, limit={}",
        query, mode, limit
    );

    let code_graph = state.code_graph.read().await;
    let symbol_embedder = XavierSymbolEmbedder::new(state.embedder.clone());

    let result = match mode.as_str() {
        "bm25" | "fts" => code_graph.query.search(&query, limit),
        "semantic" => {
            code_graph
                .query
                .semantic_search(&query, &symbol_embedder, limit)
                .await
        }
        _ => {
            code_graph
                .query
                .hybrid_search(&query, &symbol_embedder, limit)
                .await
        }
    };

    match result {
        Ok(query_res) => {
            let results: Vec<_> = query_res
                .symbols
                .into_iter()
                .map(|symbol| {
                    serde_json::json!({
                        "id": symbol.id,
                        "stable_id": symbol.stable_id,
                        "path": symbol.file_path,
                        "symbol": symbol.name,
                        "symbol_type": format!("{:?}", symbol.kind),
                        "language": format!("{:?}", symbol.lang),
                        "line": symbol.start_line,
                        "end_line": symbol.end_line,
                        "signature": symbol.signature,
                        "parent": symbol.parent,
                        "complexity": symbol.complexity,
                    })
                })
                .collect();

            axum::Json(serde_json::json!({
                "status": "ok",
                "query": query,
                "mode": mode,
                "count": results.len(),
                "query_time_ms": query_res.query_time_ms,
                "results": results,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

/// Code stats handler.
///
/// With XAV-01 repo params (`project_id` + `root` + `indexed_commit`) the
/// stats come from THAT repo's `<root>/.xavier/code_graph.db`. A missing or
/// empty index returns degraded stats for the requested repo — never another
/// repo's graph. Without params, legacy workspace behavior is preserved.
pub async fn code_stats_handler(
    State(state): State<CliState>,
    Query(q): Query<CodeStatsQuery>,
) -> impl axum::response::IntoResponse {
    if q.root.as_deref().is_some_and(|s| !s.trim().is_empty()) {
        let Some(r) = resolve_requested_repo(q.project_id.as_deref(), q.root.as_deref()) else {
            let root = q.root.as_deref().unwrap_or("");
            return degraded_stats_response(
                q.project_id.as_deref().unwrap_or("unknown"),
                root,
                q.indexed_commit.as_deref().unwrap_or("unknown"),
                "missing",
                format!(
                    "No hay índice CodeGraph para '{}' (repo sin .xavier/code_graph.db). Ejecuta `xavier code scan .` en ese repo.",
                    root
                ),
            );
        };
        if !r.db_path.exists() {
            // Same-repo migration: the daemon's pre-XAV-01 global index IS
            // this repo's index (no per-repo file yet). Any other repo gets
            // degraded — never the workspace's graph.
            if is_workspace_repo(&state.workspace_dir, &r) {
                let body = legacy_stats_json(&state).await;
                return axum::Json(with_repo_echo(body, &r, true));
            }
            return degraded_stats_response(
                &r.project_id,
                &r.root.to_string_lossy(),
                &r.indexed_commit,
                "missing",
                format!(
                    "No hay índice CodeGraph para '{}' (falta {}). Ejecuta `xavier code scan .` en ese repo.",
                    r.root.display(),
                    r.db_path.display()
                ),
            );
        }
        let db_stats: Result<code_graph::types::IndexStats, String> = (|| {
            let db = ConnectionManager::global()
                .get_code_graph_db(&r.db_path)
                .map_err(|e| e.to_string())?;
            db.stats().map_err(|e| e.to_string())
        })();
        return match db_stats {
            Ok(stats) if stats.total_symbols > 0 => {
                let body = serde_json::json!({
                    "status": "ok",
                    "total_files": stats.total_files,
                    "total_symbols": stats.total_symbols,
                    "total_imports": stats.total_imports,
                    "languages": stats.languages,
                    "degraded": false,
                });
                axum::Json(with_repo_echo(body, &r, false))
            }
            Ok(_) => degraded_stats_response(
                &r.project_id,
                &r.root.to_string_lossy(),
                &r.indexed_commit,
                "empty",
                format!(
                    "CodeGraph vacío para '{}' (total_symbols=0). Ejecuta `xavier code scan .` en ese repo.",
                    r.root.display()
                ),
            ),
            Err(error) => degraded_stats_response(
                &r.project_id,
                &r.root.to_string_lossy(),
                &r.indexed_commit,
                "unreadable",
                format!(
                    "No se pudo leer el índice de '{}': {}",
                    r.root.display(),
                    error
                ),
            ),
        };
    }

    axum::Json(legacy_stats_json(&state).await)
}

/// Git-driven CodeGraph sync handler (`POST /code/sync`).
pub async fn code_sync_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    let git = payload.get("git").and_then(|v| v.as_bool()).unwrap_or(true);
    if !git {
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "Usa git=true (xavier code sync --git)",
        }));
    }
    let base = payload
        .get("base")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let staged = payload
        .get("staged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let with_memory = payload
        .get("memory")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Prefer a real git root: service cwd (systemd WorkingDirectory) is often
    // the repo, while workspace_dir may be a parent path without `.git`.
    let sync_workspace = {
        let cwd = std::env::current_dir().unwrap_or_else(|_| state.workspace_dir.clone());
        crate::cli::codegraph_sync::find_git_root(&cwd)
            .or_else(|| crate::cli::codegraph_sync::find_git_root(&state.workspace_dir))
            .or_else(|| {
                crate::cli::codegraph_sync::find_git_root(&state.workspace_dir.join("xavier"))
            })
            .unwrap_or(cwd)
    };

    let opts = crate::cli::codegraph_sync::GitSyncOptions {
        workspace: sync_workspace,
        base,
        staged,
        with_memory,
    };

    let code_graph = state.code_graph.read().await;
    match crate::cli::codegraph_sync::sync_codegraph_with_state(&code_graph, &opts.workspace, &opts)
        .await
    {
        Ok(result) => axum::Json(serde_json::to_value(&result).unwrap_or_else(|_| {
            serde_json::json!({
                "status": "error",
                "message": "failed to serialize sync result"
            })
        })),
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

/// Code context handler.
pub async fn code_context_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeContextPayload>,
) -> impl axum::response::IntoResponse {
    let sec_result = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "code/context blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let limit = payload.limit.clamp(1, 100);
    let kind_limit = if payload.query.trim().is_empty() {
        limit
    } else {
        10_000
    };
    let budget_tokens = payload.budget_tokens.clamp(100, 8000);

    let code_graph = state.code_graph.read().await;
    let mut symbols = if let Some(kind) = payload.kind.as_deref() {
        match kind.to_ascii_lowercase().as_str() {
            "function" | "fn" => code_graph.query.functions(kind_limit).unwrap_or_default(),
            "struct" => code_graph.query.structs(kind_limit).unwrap_or_default(),
            "class" => code_graph.query.classes(kind_limit).unwrap_or_default(),
            "enum" => code_graph.query.enums(kind_limit).unwrap_or_default(),
            "route" | "http_route" => code_graph.query.routes(kind_limit).unwrap_or_default(),
            _ => code_graph
                .query
                .search(&payload.query, limit)
                .map(|result| result.symbols)
                .unwrap_or_default(),
        }
    } else {
        code_graph
            .query
            .search(&payload.query, limit)
            .map(|result| result.symbols)
            .unwrap_or_default()
    };
    // filter_symbols_by_query removed — not available in code_graph API
    // The search already filters by query via QueryEngine::search
    symbols.truncate(limit);

    let mut used_tokens = 0usize;
    let mut context = Vec::new();

    for symbol in symbols {
        let signature = symbol.signature.clone().unwrap_or_default();

        let mut references = Vec::new();
        if let Some(ref stable_id) = symbol.stable_id {
            if let Ok(edges) = code_graph.db.find_edges_to(stable_id, None, 100) {
                for edge in edges {
                    references.push(serde_json::json!({
                        "from_symbol": edge.from_symbol,
                        "edge_type": format!("{:?}", edge.edge_type),
                        "path": edge.file_path,
                        "line": edge.line,
                        "confidence": edge.confidence,
                        "metadata": edge.metadata,
                    }));
                }
            }
        }

        let compact = serde_json::json!({
            "symbol": symbol.name,
            "symbol_type": format!("{:?}", symbol.kind),
            "language": format!("{:?}", symbol.lang),
            "path": symbol.file_path,
            "line": symbol.start_line,
            "end_line": symbol.end_line,
            "signature": signature,
            "stable_id": symbol.stable_id,
            "complexity": symbol.complexity,
            "references": references,
        });
        let estimated = estimate_tokens(&compact.to_string());
        if used_tokens + estimated > budget_tokens && !context.is_empty() {
            break;
        }
        used_tokens += estimated;
        context.push(compact);
    }

    axum::Json(serde_json::json!({
        "status": "ok",
        "query": payload.query,
        "budget_tokens": budget_tokens,
        "estimated_tokens": used_tokens,
        "count": context.len(),
        "context": context,
    }))
}

/// Code dependencies handler.
pub async fn code_dependencies_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, false, false).await
}

/// Code reverse dependencies handler.
pub async fn code_reverse_dependencies_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, true, false).await
}

/// Code call chain handler.
pub async fn code_call_chain_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, false, true).await
}

/// Code blast radius handler.
pub async fn code_blast_radius_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeBlastRadiusPayload>,
) -> impl axum::response::IntoResponse {
    let name_or_query = payload
        .name
        .as_deref()
        .or(payload.query.as_deref())
        .unwrap_or("")
        .to_string();

    let sec_result = state
        .security
        .process_input(&name_or_query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: name_or_query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let query = sec_result
        .sanitized_input
        .unwrap_or_else(|| sec_result.original_input.clone());

    let depth = payload.depth.clamp(1, 8);
    let code_graph = state.code_graph.read().await;

    match code_graph.query.blast_radius(&query, depth) {
        Ok(results) => {
            let json_results: Vec<_> = results
                .into_iter()
                .map(|(sym, d)| {
                    serde_json::json!({
                        "symbol": sym.name,
                        "symbol_type": format!("{:?}", sym.kind),
                        "path": sym.file_path,
                        "line": sym.start_line,
                        "end_line": sym.end_line,
                        "depth": d,
                        "stable_id": sym.stable_id,
                        "signature": sym.signature,
                    })
                })
                .collect();

            axum::Json(serde_json::json!({
                "status": "ok",
                "name": query,
                "depth": depth,
                "count": json_results.len(),
                "results": json_results,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

/// Code hubs handler.
pub async fn code_hubs_handler(State(state): State<CliState>) -> impl axum::response::IntoResponse {
    let code_graph = state.code_graph.read().await;
    match code_graph
        .query
        .hubs(default_min_degree(), default_graph_limit())
    {
        Ok(hubs) => {
            let (items, truncated, estimated_tokens) =
                truncate_json_items(hubs, default_graph_budget());
            axum::Json(serde_json::json!({
                "status": "ok",
                "count": items.len(),
                "min_degree": default_min_degree(),
                "estimated_tokens": estimated_tokens,
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

/// Code hotspots handler.
pub async fn code_hotspots_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let code_graph = state.code_graph.read().await;
    match code_graph
        .query
        .hotspots(default_min_complexity(), default_graph_limit())
    {
        Ok(hotspots) => {
            let (items, truncated, estimated_tokens) =
                truncate_json_items(hotspots, default_graph_budget());
            axum::Json(serde_json::json!({
                "status": "ok",
                "count": items.len(),
                "min_complexity": default_min_complexity(),
                "estimated_tokens": estimated_tokens,
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

async fn code_graph_edges_response(
    state: &CliState,
    payload: CodeGraphQueryPayload,
    reverse: bool,
    call_chain: bool,
) -> axum::Json<serde_json::Value> {
    let sec_result = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let query = sec_result
        .sanitized_input
        .unwrap_or_else(|| sec_result.original_input.clone());
    let edge_type = if call_chain {
        Some(::code_graph::types::EdgeType::Calls)
    } else {
        match parse_code_edge_type(payload.edge_type.as_deref()) {
            Ok(edge_type) => edge_type,
            Err(message) => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": message,
                }))
            }
        }
    };
    let depth = payload.depth.clamp(1, 8);
    let limit = payload.limit.clamp(1, 1000);
    let budget_tokens = payload.budget_tokens.clamp(100, 16_000);

    let code_graph = state.code_graph.read().await;
    let result = if call_chain {
        code_graph.query.call_chain(&query, depth, limit)
    } else if reverse {
        code_graph
            .query
            .reverse_dependencies(&query, edge_type, depth, limit)
    } else {
        code_graph
            .query
            .dependencies(&query, edge_type, depth, limit)
    };

    match result {
        Ok(edges) => {
            let (items, truncated, estimated_tokens) = truncate_json_items(edges, budget_tokens);
            axum::Json(serde_json::json!({
                "status": "ok",
                "query": query,
                "depth": depth,
                "limit": limit,
                "budget_tokens": budget_tokens,
                "estimated_tokens": estimated_tokens,
                "count": items.len(),
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

fn code_find_symbols(
    code_query: &code_graph::query::QueryEngine,
    query: &str,
    name: Option<&str>,
    kind: Option<&str>,
    pattern: Option<&str>,
    limit: usize,
) -> Vec<code_graph::types::Symbol> {
    let limit = limit.clamp(1, 100);

    let broad_limit = if query.trim().is_empty() && name.is_none() {
        limit
    } else {
        10_000
    };

    let mut symbols = if let Some(n) = name.filter(|s| !s.trim().is_empty()) {
        code_query.find_by_name(n, broad_limit).unwrap_or_default()
    } else if let Some(pattern) = pattern.filter(|p| !p.trim().is_empty()) {
        if is_supported_code_pattern(pattern) {
            code_query
                .search_by_pattern(pattern, broad_limit)
                .unwrap_or_default()
        } else {
            search_code_symbols_with_fallback(code_query, pattern, broad_limit)
        }
    } else if let Some(k) = kind.filter(|k| !k.trim().is_empty()) {
        match k.to_ascii_lowercase().as_str() {
            "function" | "fn" => code_query.functions(broad_limit).unwrap_or_default(),
            "struct" => code_query.structs(broad_limit).unwrap_or_default(),
            "class" => code_query.classes(broad_limit).unwrap_or_default(),
            "enum" => code_query.enums(broad_limit).unwrap_or_default(),
            "route" | "http_route" => code_query.routes(broad_limit).unwrap_or_default(),
            _ => search_code_symbols_with_fallback(code_query, query, broad_limit),
        }
    } else {
        search_code_symbols_with_fallback(code_query, query, broad_limit)
    };

    // Post-filter by query if supplied
    filter_symbols_by_query(&mut symbols, query);

    // Post-filter by kind if supplied (handles all SymbolKind variants)
    if let Some(k) = kind.filter(|k| !k.trim().is_empty()) {
        filter_symbols_by_kind(&mut symbols, k);
    }

    symbols.truncate(limit);
    symbols
}

fn filter_symbols_by_kind(symbols: &mut Vec<code_graph::types::Symbol>, kind_str: &str) {
    let target = kind_str.trim().to_ascii_lowercase();
    symbols.retain(|symbol| {
        let actual = format!("{:?}", symbol.kind).to_ascii_lowercase();
        match target.as_str() {
            "function" | "fn" => actual == "function",
            "struct" => actual == "struct",
            "enum" => actual == "enum",
            "class" => actual == "class",
            "method" => actual == "method",
            "variable" | "var" => actual == "variable",
            "constant" | "const" => actual == "constant",
            "trait" => actual == "trait",
            "impl" => actual == "impl",
            "module" | "mod" => actual == "module",
            "file" => actual == "file",
            "import" => actual == "import",
            "export" => actual == "export",
            "route" | "http_route" => actual == "route",
            "component" => actual == "component",
            "property" => actual == "property",
            "field" => actual == "field",
            "parameter" | "param" => actual == "parameter",
            "typealias" | "type_alias" | "type" => actual == "typealias",
            "namespace" => actual == "namespace",
            other => actual == other,
        }
    });
}

fn is_supported_code_pattern(pattern: &str) -> bool {
    matches!(
        pattern,
        "function_call"
            | "function_definition"
            | "struct_definition"
            | "struct"
            | "class_definition"
            | "class"
            | "enum_definition"
            | "enum"
            | "route_definition"
            | "route"
            | "http_route"
            | "module_definition"
            | "module"
            | "import"
            | "use_statement"
    )
}

fn search_code_symbols_with_fallback(
    code_query: &code_graph::query::QueryEngine,
    query: &str,
    limit: usize,
) -> Vec<code_graph::types::Symbol> {
    let query = query.trim();
    let mut symbols = code_query
        .search(query, limit)
        .map(|result| result.symbols)
        .unwrap_or_default();

    if symbols.is_empty() {
        if let Some(token) = best_symbol_query_token(query) {
            if token != query {
                symbols = code_query
                    .search(token, limit)
                    .map(|result| result.symbols)
                    .unwrap_or_default();
            }
        }
    }
    symbols
}

fn best_symbol_query_token(query: &str) -> Option<&str> {
    query
        .split(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
        .filter(|token| !token.is_empty())
        .filter(|token| {
            !matches!(
                token.to_ascii_lowercase().as_str(),
                "fn" | "function" | "struct" | "class" | "enum" | "async" | "pub"
            )
        })
        .max_by_key(|token| token.len())
}

fn filter_symbols_by_query(symbols: &mut Vec<code_graph::types::Symbol>, query: &str) {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return;
    }

    let query_words: Vec<&str> = query.split_whitespace().filter(|w| w.len() >= 3).collect();

    symbols.retain(|symbol| {
        let name = symbol.name.to_ascii_lowercase();
        let sig = symbol
            .signature
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let path = symbol.file_path.to_ascii_lowercase();

        // Standard substring contains
        if name.contains(&query) || sig.contains(&query) || path.contains(&query) {
            return true;
        }

        // Fuzzy/multi-word contains
        if !query_words.is_empty() {
            return query_words
                .iter()
                .any(|word| name.contains(word) || sig.contains(word) || path.contains(word));
        }

        false
    });
}

fn parse_code_edge_type(
    value: Option<&str>,
) -> std::result::Result<Option<::code_graph::types::EdgeType>, String> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "calls" | "call" => Ok(Some(::code_graph::types::EdgeType::Calls)),
        "defines" | "define" => Ok(Some(::code_graph::types::EdgeType::Defines)),
        "uses" | "use" => Ok(Some(::code_graph::types::EdgeType::Uses)),
        "imports" | "import" => Ok(Some(::code_graph::types::EdgeType::Imports)),
        "contains" | "contain" => Ok(Some(::code_graph::types::EdgeType::Contains)),
        "references" | "reference" | "refs" => Ok(Some(::code_graph::types::EdgeType::References)),
        _ => Err(format!("unsupported edge_type: {}", value)),
    }
}

fn truncate_json_items<T: Serialize>(
    items: Vec<T>,
    budget_tokens: usize,
) -> (Vec<serde_json::Value>, bool, usize) {
    let mut output = Vec::new();
    let mut used_tokens = 0usize;
    let mut truncated = false;

    for item in items {
        let value = serde_json::to_value(item).unwrap_or_else(|_| serde_json::json!({}));
        let estimated = estimate_tokens(&value.to_string());
        if used_tokens + estimated > budget_tokens && !output.is_empty() {
            truncated = true;
            break;
        }
        used_tokens += estimated;
        output.push(value);
    }

    (output, truncated, used_tokens)
}

/// Code graph view handler.
pub async fn code_graph_view_handler(
    State(state): State<CliState>,
    axum::extract::Query(params): axum::extract::Query<CodeGraphViewParams>,
) -> impl axum::response::IntoResponse {
    let edge_type_str = match secure_optional_request_field(
        state.security.as_ref(),
        "code/graph/view edge_type",
        params.edge_type.as_deref(),
    )
    .await
    {
        Ok(et) => et,
        Err(sec_result) => {
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "edge_type",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };

    let edge_type = match parse_code_edge_type(edge_type_str.as_deref()) {
        Ok(et) => et,
        Err(message) => {
            return axum::Json(serde_json::json!({
                "status": "error",
                "message": message,
            }))
        }
    };

    let depth = params.depth.clamp(1, 8);
    let limit = params.limit.clamp(1, 1000);

    let code_graph = state.code_graph.read().await;
    let total_symbols = code_graph.db.stats().map(|s| s.total_symbols).unwrap_or(0);

    let mut seed_symbols = Vec::new();
    let mut candidate_edges = Vec::new();

    if params.mode == "ego" {
        let query_param = match params.query.as_ref() {
            Some(q) => q,
            None => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": "query is required for ego mode",
                }))
            }
        };

        let sec_result = state
            .security
            .process_input(query_param)
            .await
            .unwrap_or_else(|_| SecureInputResult {
                allowed: false,
                sanitized_input: None,
                original_input: query_param.clone(),
                detection_confidence: 1.0,
                is_injection: true,
                attack_type: "unknown".to_string(),
            });

        if !sec_result.allowed {
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }

        let query = sec_result
            .sanitized_input
            .unwrap_or_else(|| sec_result.original_input.clone());

        if query.len() == 64 && query.chars().all(|ch| ch.is_ascii_hexdigit()) {
            if let Ok(Some(sym)) = code_graph.db.symbol_by_stable_id(&query) {
                seed_symbols.push(sym);
            }
        } else {
            if let Ok(result) = code_graph.db.find_symbols(&query, 1) {
                if let Some(sym) = result.symbols.into_iter().next() {
                    seed_symbols.push(sym);
                }
            }
        }

        let edges_res = if edge_type == Some(::code_graph::types::EdgeType::Calls) {
            code_graph.query.call_chain(&query, depth, limit)
        } else {
            code_graph
                .query
                .dependencies(&query, edge_type, depth, limit)
        };

        match edges_res {
            Ok(edges) => candidate_edges = edges,
            Err(err) => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": err.to_string(),
                }))
            }
        }
    } else {
        // default to "overview"
        let hubs = match code_graph.query.hubs(params.min_degree, limit) {
            Ok(h) => h,
            Err(err) => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": err.to_string(),
                }))
            }
        };

        seed_symbols = hubs.into_iter().map(|h| h.symbol).collect();

        for sym in &seed_symbols {
            if let Some(ref stable_id) = sym.stable_id {
                if let Ok(from_edges) =
                    code_graph
                        .db
                        .find_edges_from(stable_id, edge_type.clone(), limit)
                {
                    candidate_edges.extend(from_edges);
                }
                if let Ok(to_edges) =
                    code_graph
                        .db
                        .find_edges_to(stable_id, edge_type.clone(), limit)
                {
                    candidate_edges.extend(to_edges);
                }
            }
        }
    }

    let (nodes, links, truncated) = map_edges_to_graph(
        seed_symbols,
        candidate_edges,
        params.include_file_nodes,
        limit,
        &code_graph.db,
    );

    let shown_nodes = nodes.len();
    let shown_links = links.len();

    axum::Json(serde_json::json!({
        "status": "ok",
        "layer": "code",
        "truncated": truncated,
        "nodes": nodes,
        "links": links,
        "stats": {
            "total_symbols": total_symbols,
            "shown_nodes": shown_nodes,
            "shown_links": shown_links,
        }
    }))
}

fn map_edges_to_graph(
    seed_symbols: Vec<Symbol>,
    candidate_edges: Vec<CodeEdge>,
    include_file_nodes: bool,
    limit: usize,
    db: &::code_graph::db::CodeGraphDB,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>, bool) {
    let mut symbol_map: std::collections::HashMap<String, Symbol> =
        std::collections::HashMap::new();
    let mut seed_symbols_filtered = Vec::new();

    for sym in seed_symbols {
        if let Some(ref stable_id) = sym.stable_id {
            if !include_file_nodes
                && (stable_id.starts_with("file:") || stable_id.starts_with("module:"))
            {
                continue;
            }
            symbol_map.insert(stable_id.clone(), sym.clone());
            seed_symbols_filtered.push(sym);
        }
    }

    let mut filtered_edges = Vec::new();
    let mut edge_keys = std::collections::HashSet::new();

    for edge in candidate_edges {
        let from = &edge.from_symbol;
        let to = &edge.to_symbol;

        if !include_file_nodes
            && (from.starts_with("file:")
                || from.starts_with("module:")
                || to.starts_with("file:")
                || to.starts_with("module:"))
        {
            continue;
        }

        let edge_key = (
            from.clone(),
            to.clone(),
            edge.edge_type.as_str().to_string(),
        );
        if !edge_keys.insert(edge_key) {
            continue;
        }

        filtered_edges.push(edge);
    }

    let mut shown_node_ids = std::collections::HashSet::new();
    let mut final_nodes = Vec::new();

    for sym in seed_symbols_filtered {
        if let Some(ref stable_id) = sym.stable_id {
            if final_nodes.len() >= limit {
                break;
            }
            if shown_node_ids.insert(stable_id.clone()) {
                final_nodes.push(sym);
            }
        }
    }

    let mut accepted_edges = Vec::new();
    let mut pending_edges = Vec::new();

    for edge in filtered_edges {
        let from = &edge.from_symbol;
        let to = &edge.to_symbol;

        let from_in = shown_node_ids.contains(from);
        let to_in = shown_node_ids.contains(to);

        if from_in && to_in {
            accepted_edges.push(edge);
        } else if from_in || to_in {
            pending_edges.push(edge);
        }
    }

    for edge in pending_edges {
        let from = &edge.from_symbol;
        let to = &edge.to_symbol;

        let from_in = shown_node_ids.contains(from);
        let neighbor_id = if from_in { to } else { from };

        if shown_node_ids.contains(neighbor_id) {
            accepted_edges.push(edge);
            continue;
        }

        if final_nodes.len() < limit {
            let sym_opt = if let Ok(Some(sym)) = db.symbol_by_stable_id(neighbor_id) {
                Some(sym)
            } else {
                if neighbor_id.starts_with("file:") || neighbor_id.starts_with("module:") {
                    let parts: Vec<&str> = neighbor_id.splitn(2, ':').collect();
                    let name = parts.get(1).unwrap_or(&neighbor_id.as_str()).to_string();
                    let kind = if neighbor_id.starts_with("file:") {
                        SymbolKind::File
                    } else {
                        SymbolKind::Module
                    };
                    Some(Symbol {
                        id: None,
                        stable_id: Some(neighbor_id.clone()),
                        name,
                        kind,
                        lang: ::code_graph::types::Language::Unknown,
                        file_path: "".to_string(),
                        start_line: 0,
                        end_line: 0,
                        start_col: 0,
                        end_col: 0,
                        signature: None,
                        parent: None,
                        complexity: None,
                    })
                } else {
                    None
                }
            };

            if let Some(sym) = sym_opt {
                shown_node_ids.insert(neighbor_id.clone());
                final_nodes.push(sym);
                accepted_edges.push(edge);
            }
        }
    }

    let mut all_possible_node_ids = std::collections::HashSet::new();
    for id in symbol_map.keys() {
        all_possible_node_ids.insert(id.clone());
    }
    for edge in &accepted_edges {
        all_possible_node_ids.insert(edge.from_symbol.clone());
        all_possible_node_ids.insert(edge.to_symbol.clone());
    }
    let truncated = all_possible_node_ids.len() > limit;

    let nodes_json: Vec<serde_json::Value> = final_nodes
        .into_iter()
        .map(|sym| {
            let id = sym.stable_id.unwrap_or_default();
            let label = sym.name;
            let kind = format!("{:?}", sym.kind);
            let meta = serde_json::json!({
                "path": sym.file_path,
                "line": sym.start_line,
                "lang": format!("{:?}", sym.lang),
                "complexity": sym.complexity.unwrap_or(0.0),
            });
            serde_json::json!({
                "id": id,
                "label": label,
                "kind": kind,
                "meta": meta,
            })
        })
        .collect();

    let links_json: Vec<serde_json::Value> = accepted_edges
        .into_iter()
        .map(|edge| {
            serde_json::json!({
                "source": edge.from_symbol,
                "target": edge.to_symbol,
                "relation": edge.edge_type.as_str(),
                "weight": edge.confidence as f64,
            })
        })
        .collect();

    (nodes_json, links_json, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_graph::db::CodeGraphDB;
    use code_graph::types::{CodeEdge, EdgeType, Language, Symbol, SymbolKind};

    #[test]
    fn test_filter_symbols_by_query_strict_filtering_bug_fixed_fixed() {
        let mut symbols = vec![Symbol {
            id: None,
            stable_id: None,
            name: "CacheStore".to_string(),
            kind: SymbolKind::Struct,
            lang: Language::Rust,
            file_path: "src/cache.rs".to_string(),
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            signature: Some("struct CacheStore".to_string()),
            parent: None,
            complexity: None,
        }];

        // The query "cache store" now matches because it is split into words "cache" and "store"
        filter_symbols_by_query(&mut symbols, "cache store");
        assert_eq!(symbols.len(), 1);
    }

    #[test]
    fn test_filter_symbols_by_query_case_insensitive() {
        let mut symbols = vec![Symbol {
            id: None,
            stable_id: None,
            name: "CacheStore".to_string(),
            kind: SymbolKind::Struct,
            lang: Language::Rust,
            file_path: "src/cache.rs".to_string(),
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            signature: Some("struct CacheStore".to_string()),
            parent: None,
            complexity: None,
        }];

        filter_symbols_by_query(&mut symbols, "cache");
        assert_eq!(symbols.len(), 1);
    }

    #[test]
    fn test_code_find_symbols_kind_filtering() {
        let db = std::sync::Arc::new(CodeGraphDB::in_memory().unwrap());
        let s1 = Symbol {
            id: Some(1),
            stable_id: Some("stable-1".to_string()),
            name: "func_one".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/one.rs".to_string(),
            start_line: 10,
            end_line: 20,
            start_col: 1,
            end_col: 1,
            signature: None,
            parent: None,
            complexity: Some(1.5),
        };
        let s2 = Symbol {
            id: Some(2),
            stable_id: Some("stable-2".to_string()),
            name: "struct_two".to_string(),
            kind: SymbolKind::Struct,
            lang: Language::Rust,
            file_path: "src/two.rs".to_string(),
            start_line: 30,
            end_line: 40,
            start_col: 1,
            end_col: 1,
            signature: None,
            parent: None,
            complexity: Some(2.5),
        };
        db.insert_symbol(&s1).unwrap();
        db.insert_symbol(&s2).unwrap();

        let query_engine = code_graph::query::QueryEngine::new(db.clone());

        // Filter by kind "function"
        let res = code_find_symbols(&query_engine, "func", None, Some("function"), None, 10);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].name, "func_one");

        // Filter by kind "struct"
        let res = code_find_symbols(&query_engine, "struct", None, Some("struct"), None, 10);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].name, "struct_two");

        // Additional symbol kinds (Method, Variable)
        let s3 = Symbol {
            id: Some(3),
            stable_id: Some("stable-3".to_string()),
            name: "method_three".to_string(),
            kind: SymbolKind::Method,
            lang: Language::Rust,
            file_path: "src/three.rs".to_string(),
            start_line: 1,
            end_line: 5,
            start_col: 1,
            end_col: 1,
            signature: None,
            parent: Some("struct_two".to_string()),
            complexity: Some(1.0),
        };
        db.insert_symbol(&s3).unwrap();

        let res_method = code_find_symbols(&query_engine, "", None, Some("method"), None, 10);
        assert_eq!(res_method.len(), 1);
        assert_eq!(res_method[0].name, "method_three");

        // Combination of name and kind
        let res_comb = code_find_symbols(
            &query_engine,
            "",
            Some("method_three"),
            Some("method"),
            None,
            10,
        );
        assert_eq!(res_comb.len(), 1);

        let res_mismatch = code_find_symbols(
            &query_engine,
            "",
            Some("method_three"),
            Some("struct"),
            None,
            10,
        );
        assert_eq!(res_mismatch.len(), 0);
    }

    #[test]
    fn test_map_edges_to_graph() {
        let db = CodeGraphDB::in_memory().unwrap();

        let s1 = Symbol {
            id: Some(1),
            stable_id: Some("stable-1".to_string()),
            name: "func_one".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/one.rs".to_string(),
            start_line: 10,
            end_line: 20,
            start_col: 1,
            end_col: 1,
            signature: None,
            parent: None,
            complexity: Some(1.5),
        };

        let s2 = Symbol {
            id: Some(2),
            stable_id: Some("stable-2".to_string()),
            name: "func_two".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/two.rs".to_string(),
            start_line: 30,
            end_line: 40,
            start_col: 1,
            end_col: 1,
            signature: None,
            parent: None,
            complexity: Some(2.5),
        };

        db.insert_symbol(&s1).unwrap();
        db.insert_symbol(&s2).unwrap();

        let seed_symbols = vec![s1.clone()];
        let candidate_edges = vec![CodeEdge {
            id: Some(1),
            from_symbol: "stable-1".to_string(),
            to_symbol: "stable-2".to_string(),
            edge_type: EdgeType::Calls,
            file_path: "src/one.rs".to_string(),
            line: 15,
            confidence: 1.0,
            metadata: None,
        }];

        // Test with include_file_nodes = false
        let (nodes, links, truncated) =
            map_edges_to_graph(seed_symbols, candidate_edges, false, 10, &db);

        assert!(!truncated);
        assert_eq!(nodes.len(), 2);
        assert_eq!(links.len(), 1);

        assert_eq!(nodes[0]["id"], "stable-1");
        assert_eq!(nodes[0]["label"], "func_one");
        assert_eq!(nodes[0]["kind"], "Function");
        assert_eq!(nodes[0]["meta"]["complexity"], 1.5);

        assert_eq!(nodes[1]["id"], "stable-2");
        assert_eq!(nodes[1]["label"], "func_two");
        assert_eq!(nodes[1]["kind"], "Function");
        assert_eq!(nodes[1]["meta"]["complexity"], 2.5);

        assert_eq!(links[0]["source"], "stable-1");
        assert_eq!(links[0]["target"], "stable-2");
        assert_eq!(links[0]["relation"], "Calls");
        assert_eq!(links[0]["weight"], 1.0);
    }

    #[tokio::test]
    async fn test_code_memories_linking_integration() {
        use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
        use xavier::memory::store::{MemoryRecord, MemoryStore};

        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_code_mem.db");
        let config = VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: 3,
        };

        // Hermetic linking: store.put() routes memory->symbol auto-linking
        // through code_graph_db_path_for("."), which on dev machines may
        // resolve to a pre-existing (possibly empty)
        // ~/.local/share/xavier/code_graph.db. Redirect
        // XAVIER_CODE_GRAPH_DB_PATH to a temp code graph seeded with the real
        // `require_permission` middleware symbol (src/middleware/auth.rs) so
        // the exact-name matching path is deterministic.
        let code_graph_path = temp_dir.path().join("code_graph.db");
        {
            let cg = CodeGraphDB::new(&code_graph_path).unwrap();
            cg.insert_symbol(&Symbol {
                id: None,
                // Explicit stable_id: empty stable_ids get replaced by a
                // deterministic sha256 during insert_symbol normalization,
                // and links store stable_id (falling back to name only when
                // stable_id is empty in the DB row).
                stable_id: Some("require_permission".to_string()),
                name: "require_permission".to_string(),
                kind: SymbolKind::Function,
                lang: Language::Rust,
                file_path: "src/middleware/auth.rs".to_string(),
                start_line: 23,
                end_line: 30,
                start_col: 1,
                end_col: 1,
                signature: Some("pub fn require_permission(...)".to_string()),
                parent: None,
                complexity: None,
            })
            .unwrap();
        }
        let prev_cg_path = std::env::var("XAVIER_CODE_GRAPH_DB_PATH").ok();
        std::env::set_var("XAVIER_CODE_GRAPH_DB_PATH", &code_graph_path);

        let store = VecSqliteMemoryStore::new(config).await.unwrap();

        let memory = MemoryRecord {
            id: "agent_mem_123".to_string(),
            workspace_id: "default".to_string(),
            path: "agent_memory://cursor/session-1".to_string(),
            content: "Discussed RBAC enforcement using require_permission middleware.".to_string(),
            ..Default::default()
        };

        store.put(memory).await.unwrap();

        let symbols = store.symbols_for_memory("agent_mem_123").await.unwrap();

        match prev_cg_path {
            Some(p) => std::env::set_var("XAVIER_CODE_GRAPH_DB_PATH", p),
            None => std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH"),
        }

        assert!(symbols.contains(&"require_permission".to_string()));
    }

    // ── XAV-01 handler-level isolation ────────────────────────────────
    // Stats/find with explicit repo identity resolve THAT repo's
    // `<root>/.xavier/code_graph.db`; a missing index degrades for the
    // requested repo instead of leaking another repo's graph.

    use crate::cli::state::CliState;
    use std::collections::HashMap;
    use tokio::sync::RwLock as AsyncRwLock;
    use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
    use xavier::agents::rate_limit::RateLimitManager;
    use xavier::app::proxy_use_case::ProxyUseCase;
    use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
    use xavier::codebase::conversations_db::ConversationsDb;
    use xavier::coordination::KeyLendingEngine;
    use xavier::coordination::SimpleAgentRegistry;
    use xavier::embedding::NoopEmbedder;
    use xavier::memory::agent_indexer::AgentIndexer;
    use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
    use xavier::memory::qmd_memory::QmdMemory;
    use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
    use xavier::ports::inbound::AgentLifecyclePort;
    use xavier::secrets::audit::QmdAuditLogger;
    use xavier::security::auth_store::AuthStore;
    use xavier::tasks::store::{InMemoryTaskStore, TaskService};

    async fn xav01_test_state(workspace_dir: std::path::PathBuf) -> CliState {
        let auth_store = Arc::new(AuthStore::open(":memory:", [0u8; 32]).unwrap());
        let docs = Arc::new(AsyncRwLock::new(Vec::new()));
        let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
        let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(
            VecSqliteMemoryStore::new(VecSqliteStoreConfig {
                path: tmp.path().join("vec_store.db"),
                embedding_dimensions: 3,
            })
            .await
            .unwrap(),
        );
        // Keep the tempdir alive via the store path parent? No — VecSqlite
        // opens the DB eagerly; leaking the dir guard is unnecessary.
        std::mem::forget(tmp);
        let cg_db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let cg_state = Arc::new(tokio::sync::RwLock::new(
            crate::cli::state::CodeGraphState {
                db: cg_db.clone(),
                indexer: Arc::new(code_graph::indexer::Indexer::new(cg_db.clone())),
                query: Arc::new(code_graph::query::QueryEngine::new(cg_db)),
            },
        ));
        CliState {
            memory: memory_port,
            qmd_memory,
            store,
            workspace_id: "test-ws".to_string(),
            workspace_dir: workspace_dir.clone(),
            state_dir: workspace_dir,
            auth_db: None,
            code_graph: cg_state,
            security: Arc::new(xavier::app::security_service::SecurityService::new()),
            security_scan: Arc::new(xavier::app::security_service::SecurityService::new()),
            _time_store: None,
            agent_registry: SimpleAgentRegistry::new(None) as Arc<dyn AgentLifecyclePort>,
            panel_store: Arc::new(
                ConversationsDb::open_in_memory("test-project")
                    .await
                    .unwrap(),
            ),
            secrets_engine: Arc::new(KeyLendingEngine::new(Box::new(QmdAuditLogger::new()), None)),
            event_bus: xavier::coordination::XavierEventBus::new(10),
            tasks: Arc::new(TaskService::new(Arc::new(InMemoryTaskStore::new()))),
            rate_manager: Arc::new(RateLimitManager::new()),
            prompt_cache: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            http_client: reqwest::Client::new(),
            proxy_use_case: Arc::new(ProxyUseCase::new(
                Arc::new(RateLimitManager::new()),
                Arc::new(parking_lot::Mutex::new(HashMap::new())),
            )),
            usage_counters: Arc::new(xavier::observability::UsageCounters::new()),
            session_manager: Arc::new(xavier::security::sessions::SessionManager::new(60)),
            provider_router: Arc::new(tokio::sync::RwLock::new(ProviderRouter::new(
                ProviderKind::Local,
            ))),
            embedder: Arc::new(NoopEmbedder),
            agent_indexer: Arc::new(AgentIndexer::new(FileIndexer::new(
                FileIndexerConfig::default(),
                None,
            ))),
            auth_store: Some(auth_store),
            openclaw_indexer: Arc::new(crate::memory::openclaw_indexer::OpenClawAgentIndexer::new(
                Arc::new(NoopEmbedder),
            )),
            multi_db: xavier::storage::multi_db::MultiDbManager::new(),
            system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
            maloca: xavier::maloca::MalocaStore::open(
                &std::env::temp_dir().join("xavier-maloca-test-xav01"),
            ),
        }
    }

    fn xav01_seed_repo(dir: &std::path::Path, checkpoint: &str, symbols: &[(&str, &str)]) {
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join(".xavier")).unwrap();
        std::fs::write(
            dir.join(".xavier").join("codegraph-sync-commit"),
            checkpoint,
        )
        .unwrap();
        let db_path = dir.join(".xavier").join("code_graph.db");
        let cm = xavier::codebase::connection_manager::ConnectionManager::new();
        let db = cm.get_code_graph_db(&db_path).unwrap();
        for (name, file) in symbols {
            db.insert_symbol(&Symbol {
                id: None,
                stable_id: None,
                name: name.to_string(),
                kind: SymbolKind::Function,
                lang: Language::Rust,
                file_path: file.to_string(),
                start_line: 1,
                end_line: 2,
                start_col: 0,
                end_col: 0,
                signature: None,
                parent: None,
                complexity: None,
            })
            .unwrap();
        }
        cm.unload_code_graph_db(&db_path);
    }

    fn xav01_stats_query(root: &std::path::Path) -> CodeStatsQuery {
        let id = xavier::codebase::repo_identity::derive_repo_identity(root);
        CodeStatsQuery {
            project_id: Some(id.project_id),
            root: Some(id.root),
            indexed_commit: Some(id.indexed_commit),
        }
    }

    async fn xav01_body<F, R>(fut: F) -> serde_json::Value
    where
        F: std::future::Future<Output = R>,
        R: axum::response::IntoResponse,
    {
        let body = fut.await.into_response().into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_xav01_stats_isolated_per_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let dir_a = tmp.path().join("repo-alpha-xav01");
        let dir_b = tmp.path().join("repo-beta-xav01");
        xav01_seed_repo(
            &dir_a,
            "aaa111\n",
            &[
                ("Xav01HandlerAlpha", "src/alpha.rs"),
                ("Xav01HandlerExtra", "src/extra.rs"),
            ],
        );
        xav01_seed_repo(&dir_b, "bbb222\n", &[("Xav01HandlerBeta", "src/beta.rs")]);

        let state = xav01_test_state(tmp.path().to_path_buf()).await;

        let a = xav01_body(code_stats_handler(
            State(state.clone()),
            Query(xav01_stats_query(&dir_a)),
        ))
        .await;
        let b = xav01_body(code_stats_handler(
            State(state.clone()),
            Query(xav01_stats_query(&dir_b)),
        ))
        .await;
        assert_eq!(a["status"], "ok");
        assert_eq!(a["total_symbols"], 2);
        assert_eq!(a["project_id"], "repo-alpha-xav01");
        assert_eq!(a["indexed_commit"], "aaa111");
        assert_eq!(b["status"], "ok");
        assert_eq!(b["total_symbols"], 1);
        assert_eq!(b["project_id"], "repo-beta-xav01");
        assert_eq!(b["indexed_commit"], "bbb222");
        assert_ne!(a["total_symbols"], b["total_symbols"]);

        // Missing repo → degraded for THAT repo, never A's or B's numbers.
        let missing_root = tmp.path().join("repo-missing-xav01");
        let m = xav01_body(code_stats_handler(
            State(state.clone()),
            Query(xav01_stats_query(&missing_root)),
        ))
        .await;
        assert_eq!(m["degraded"], true);
        assert_eq!(m["total_symbols"], 0);
        assert!(m["root"].as_str().unwrap().contains("repo-missing-xav01"));

        println!(
            "XAV-01 stats OK: A symbols={} project={} | B symbols={} project={} | missing degraded={}",
            a["total_symbols"], a["project_id"], b["total_symbols"], b["project_id"], m["degraded"]
        );
    }

    #[tokio::test]
    async fn test_xav01_find_isolated_per_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let dir_a = tmp.path().join("repo-alpha-xav01");
        let dir_b = tmp.path().join("repo-beta-xav01");
        xav01_seed_repo(&dir_a, "aaa111\n", &[("Xav01HandlerAlpha", "src/alpha.rs")]);
        xav01_seed_repo(&dir_b, "bbb222\n", &[("Xav01HandlerBeta", "src/beta.rs")]);

        let state = xav01_test_state(tmp.path().to_path_buf()).await;
        let payload_for = |query: &str, root: &std::path::Path| {
            let id = xavier::codebase::repo_identity::derive_repo_identity(root);
            axum::Json(CodeFindPayload {
                query: query.to_string(),
                name: None,
                limit: 10,
                kind: None,
                pattern: None,
                repo: Some(RepoIdentityPayload {
                    project_id: Some(id.project_id),
                    root: Some(id.root),
                    indexed_commit: Some(id.indexed_commit),
                }),
            })
        };

        // Exclusive symbol resolves in its own repo…
        let hit = xav01_body(code_find_handler(
            State(state.clone()),
            payload_for("Xav01HandlerBeta", &dir_b),
        ))
        .await;
        assert_eq!(hit["status"], "ok");
        assert_eq!(hit["count"], 1);
        assert_eq!(hit["project_id"], "repo-beta-xav01");
        // …and is invisible from the other repo (the reported XAV-01 bug
        // returned the daemon workspace's graph instead).
        let miss = xav01_body(code_find_handler(
            State(state.clone()),
            payload_for("Xav01HandlerBeta", &dir_a),
        ))
        .await;
        assert_eq!(miss["status"], "ok");
        assert_eq!(miss["count"], 0);
        assert_eq!(miss["project_id"], "repo-alpha-xav01");

        // Missing repo → degraded, zero results, own identity echoed.
        let missing_root = tmp.path().join("repo-missing-xav01");
        let deg = xav01_body(code_find_handler(
            State(state.clone()),
            payload_for("Xav01HandlerBeta", &missing_root),
        ))
        .await;
        assert_eq!(deg["count"], 0);
        assert_eq!(deg["degraded"], true);

        println!(
            "XAV-01 find OK: beta@B count={} | beta@A count={} | beta@missing degraded={}",
            hit["count"], miss["count"], deg["degraded"]
        );
    }

    #[tokio::test]
    async fn test_xav01_same_repo_legacy_fallback() {
        // Repo WITHOUT a per-repo `<root>/.xavier/code_graph.db`, but equal
        // to the daemon workspace repo: the pre-XAV-01 global index serves
        // it (migration path). A different repo must still degrade.
        let _env = crate::settings::tests::TempEnv::new();
        let tmp = tempfile::tempdir().unwrap();
        let dir_ws = tmp.path().join("repo-ws-xav01");
        std::fs::create_dir_all(dir_ws.join(".git")).unwrap();
        std::fs::create_dir_all(dir_ws.join(".xavier")).unwrap();

        // Seed the legacy global DB file (redirected via env override) with
        // a marker symbol; the workspace repo has NO per-repo DB file.
        let legacy_db_path = tmp.path().join("legacy-global").join("code_graph.db");
        std::fs::create_dir_all(legacy_db_path.parent().unwrap()).unwrap();
        {
            let cm = xavier::codebase::connection_manager::ConnectionManager::new();
            let legacy_db = cm.get_code_graph_db(&legacy_db_path).unwrap();
            legacy_db
                .insert_symbol(&Symbol {
                    id: None,
                    stable_id: None,
                    name: "Xav01LegacyMark".to_string(),
                    kind: SymbolKind::Function,
                    lang: Language::Rust,
                    file_path: "src/legacy.rs".to_string(),
                    start_line: 1,
                    end_line: 2,
                    start_col: 0,
                    end_col: 0,
                    signature: None,
                    parent: None,
                    complexity: None,
                })
                .unwrap();
            cm.unload_code_graph_db(&legacy_db_path);
        }
        std::env::set_var("XAVIER_CODE_GRAPH_DB_PATH", &legacy_db_path);
        assert!(!dir_ws.join(".xavier").join("code_graph.db").exists());

        let state = xav01_test_state(dir_ws.clone()).await;

        let id_ws = xavier::codebase::repo_identity::derive_repo_identity(&dir_ws);
        let stats = xav01_body(code_stats_handler(
            State(state.clone()),
            Query(CodeStatsQuery {
                project_id: Some(id_ws.project_id.clone()),
                root: Some(id_ws.root.clone()),
                indexed_commit: Some(id_ws.indexed_commit.clone()),
            }),
        ))
        .await;
        assert_eq!(stats["status"], "ok");
        assert_eq!(stats["total_symbols"], 1);
        assert_eq!(stats["legacy_index"], true);
        assert_eq!(stats["project_id"], "repo-ws-xav01");

        let find = xav01_body(code_find_handler(
            State(state.clone()),
            axum::Json(CodeFindPayload {
                query: "Xav01LegacyMark".to_string(),
                name: None,
                limit: 10,
                kind: None,
                pattern: None,
                repo: Some(RepoIdentityPayload {
                    project_id: Some(id_ws.project_id.clone()),
                    root: Some(id_ws.root.clone()),
                    indexed_commit: Some(id_ws.indexed_commit.clone()),
                }),
            }),
        ))
        .await;
        assert_eq!(find["count"], 1);
        assert_eq!(find["legacy_index"], true);

        // …while another repo without an index degrades (no cross-talk).
        let other = tmp.path().join("repo-other-xav01");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        let id_other = xavier::codebase::repo_identity::derive_repo_identity(&other);
        let deg = xav01_body(code_stats_handler(
            State(state.clone()),
            Query(CodeStatsQuery {
                project_id: Some(id_other.project_id),
                root: Some(id_other.root),
                indexed_commit: Some(id_other.indexed_commit),
            }),
        ))
        .await;
        assert_eq!(deg["degraded"], true);
        assert_eq!(deg["total_symbols"], 0);

        println!(
            "XAV-01 legacy OK: ws symbols={} legacy={} | other degraded={}",
            stats["total_symbols"], stats["legacy_index"], deg["degraded"]
        );
    }

    #[tokio::test]
    async fn test_code_sidecar_status_handler() {
        let tmp = tempfile::tempdir().unwrap();
        let state = xav01_test_state(tmp.path().to_path_buf()).await;

        let res = xav01_body(code_sidecar_status_handler(State(state.clone()))).await;
        assert_eq!(res["engine"], "colby");
        assert!(res.get("available").is_some());
        assert!(res.get("message").is_some());
    }

    #[tokio::test]
    async fn test_code_sidecar_install_handler() {
        let tmp = tempfile::tempdir().unwrap();
        let state = xav01_test_state(tmp.path().to_path_buf()).await;

        let payload = axum::Json(InstallSidecarRequest {
            reprompt: Some(true),
            force: Some(false),
            mode: Some("ask".to_string()),
        });

        let res = xav01_body(code_sidecar_install_handler(
            State(state.clone()),
            Some(payload),
        ))
        .await;
        assert_eq!(res["engine"], "colby");
        assert!(res.get("status").is_some());
        assert!(res.get("available").is_some());
        assert!(res.get("message").is_some());
    }
}
