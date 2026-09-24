//! Global Unified Backlog Aggregation Endpoint.
//!
//! Path: src/server/maloca/backlog_route.rs
//!
//! Aggregates features from all SWAL product repos (`.gitcore/features.json`),
//! supporting query filters (`?wave=1`, `?status=planned`, `?priority=P0`, `?app_id=shelf`)
//! and providing overall backlog summary metrics.
//!
//! Includes an in-memory response cache with a 30-second TTL to prevent disk thrashing.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Query parameters for filtering unified backlog items.
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct BacklogQuery {
    pub wave: Option<u32>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub app_id: Option<String>,
}

/// Aggregated feature item representation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnifiedFeatureItem {
    pub id: String,
    pub name: String,
    pub app_id: String,
    pub wave: Option<u32>,
    pub status: String,
    pub priority: Option<String>,
    pub progress_pct: f64,
    pub category: Option<String>,
    pub notes: Option<String>,
    pub github_issue: Option<serde_json::Value>,
}

/// Project container for scanned features.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedProject {
    pub repo_name: String,
    pub features: Vec<UnifiedFeatureItem>,
}

/// Wave progress summary structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaveSummary {
    pub total_features: usize,
    pub completed_features: usize,
    pub progress_pct: f64,
}

/// Total progress and breakdown per wave / status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BacklogSummaryResponse {
    pub source: String,
    pub total_features: usize,
    pub completed_features: usize,
    pub overall_progress_pct: f64,
    pub waves: BTreeMap<String, WaveSummary>,
    pub status_breakdown: BTreeMap<String, usize>,
}

/// Unified backlog response wrapper.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnifiedBacklogResponse {
    pub source: String,
    pub total: usize,
    pub items: Vec<UnifiedFeatureItem>,
}

/// Internal cache state with 30-second TTL.
#[derive(Debug, Clone)]
struct CacheEntry {
    fetched_at: Instant,
    projects: Vec<UnifiedProject>,
}

/// Thread-safe service for managing unified backlog aggregation and caching.
#[derive(Debug, Clone)]
pub struct UnifiedBacklogService {
    cache: Arc<RwLock<Option<CacheEntry>>>,
    ttl: Duration,
    custom_workspace_dir: Option<PathBuf>,
    maloca_store: Option<Arc<crate::maloca::MalocaStore>>,
}

impl Default for UnifiedBacklogService {
    fn default() -> Self {
        Self::new()
    }
}

impl UnifiedBacklogService {
    /// Creates a new `UnifiedBacklogService` with default 30s TTL.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(None)),
            ttl: Duration::from_secs(30),
            custom_workspace_dir: None,
            maloca_store: None,
        }
    }

    /// Sets a custom workspace directory for testing or custom deployments.
    pub fn with_workspace_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.custom_workspace_dir = Some(path.into());
        self
    }

    /// Wires in the live `MalocaStore` so its own hand-curated `backlog` items
    /// (`store.backlog()` — things like "MalocaView dogfood en panel-ui") are
    /// folded into the unified `/v1/maloca/backlog/*` response as a synthetic
    /// `maloca` project, alongside the scanned `.gitcore/features.json` items.
    pub fn with_maloca_store(mut self, store: Arc<crate::maloca::MalocaStore>) -> Self {
        self.maloca_store = Some(store);
        self
    }

    /// Converts `MalocaStore::backlog()` (`{"source", "items":[{id,title,status}]}`)
    /// into a synthetic `UnifiedProject` so those items show up in the aggregated
    /// backlog view. Returns `None` if no store is wired in or it has no items.
    fn maloca_backlog_project(&self) -> Option<UnifiedProject> {
        let store = self.maloca_store.as_ref()?;
        let raw = store.backlog();
        let items = raw.get("items")?.as_array()?;

        let features: Vec<UnifiedFeatureItem> = items
            .iter()
            .filter_map(|item| {
                let id = item.get("id").and_then(|v| v.as_str())?.to_string();
                let name = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&id)
                    .to_string();
                let status = item
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("planned")
                    .to_string();
                let progress_pct = if matches!(
                    status.to_lowercase().as_str(),
                    "done" | "complete" | "completed" | "stable" | "implemented"
                ) {
                    100.0
                } else {
                    0.0
                };
                Some(UnifiedFeatureItem {
                    id,
                    name,
                    app_id: "maloca".to_string(),
                    wave: None,
                    status,
                    priority: None,
                    progress_pct,
                    category: Some("maloca-backlog".to_string()),
                    notes: None,
                    github_issue: None,
                })
            })
            .collect();

        if features.is_empty() {
            return None;
        }

        Some(UnifiedProject {
            repo_name: "maloca".to_string(),
            features,
        })
    }

    /// Gets or refreshes cached project features.
    pub fn get_projects(&self) -> Vec<UnifiedProject> {
        if let Ok(guard) = self.cache.read() {
            if let Some(ref entry) = *guard {
                if entry.fetched_at.elapsed() < self.ttl {
                    return entry.projects.clone();
                }
            }
        }

        // Cache miss or expired — perform scan
        let mut projects = self.scan_all_projects();
        if let Some(maloca_project) = self.maloca_backlog_project() {
            projects.push(maloca_project);
        }

        if let Ok(mut guard) = self.cache.write() {
            *guard = Some(CacheEntry {
                fetched_at: Instant::now(),
                projects: projects.clone(),
            });
        }

        projects
    }

    /// Clears the in-memory cache forcing a fresh scan on next call.
    pub fn clear_cache(&self) {
        if let Ok(mut guard) = self.cache.write() {
            *guard = None;
        }
    }

    /// Scans candidate workspace paths for `.gitcore/features.json`.
    fn scan_all_projects(&self) -> Vec<UnifiedProject> {
        let mut dirs_to_scan = Vec::new();

        if let Some(ref custom) = self.custom_workspace_dir {
            dirs_to_scan.push(custom.clone());
        } else {
            if let Ok(env_dir) = std::env::var("SWAL_WORKSPACE_DIR") {
                dirs_to_scan.push(PathBuf::from(env_dir));
            }

            let canonical_swal = PathBuf::from("/home/belal/proyectosSWAL");
            if canonical_swal.exists() {
                dirs_to_scan.push(canonical_swal);
            }

            // Always attempt current working directory as fallback
            if let Ok(cwd) = std::env::current_dir() {
                dirs_to_scan.push(cwd);
            }
        }

        let mut aggregated_projects = Vec::new();
        let mut seen_repos = std::collections::HashSet::new();

        for base_dir in dirs_to_scan {
            let projects = scan_projects_from_directory(&base_dir);
            for proj in projects {
                if !seen_repos.contains(&proj.repo_name) {
                    seen_repos.insert(proj.repo_name.clone());
                    aggregated_projects.push(proj);
                }
            }
        }

        aggregated_projects.sort_by(|a, b| a.repo_name.cmp(&b.repo_name));
        aggregated_projects
    }

    /// Filters aggregated features according to query parameters.
    pub fn get_unified_backlog(&self, query: &BacklogQuery) -> UnifiedBacklogResponse {
        let projects = self.get_projects();
        let mut matched_items = Vec::new();

        for proj in &projects {
            if let Some(ref target_app) = query.app_id {
                if !proj.repo_name.eq_ignore_ascii_case(target_app) {
                    continue;
                }
            }

            for feat in &proj.features {
                if let Some(target_wave) = query.wave {
                    if feat.wave != Some(target_wave) {
                        continue;
                    }
                }

                if let Some(ref target_status) = query.status {
                    if !feat.status.eq_ignore_ascii_case(target_status) {
                        continue;
                    }
                }

                if let Some(ref target_priority) = query.priority {
                    let match_prio = feat
                        .priority
                        .as_ref()
                        .map(|p| p.eq_ignore_ascii_case(target_priority))
                        .unwrap_or(false);
                    if !match_prio {
                        continue;
                    }
                }

                matched_items.push(feat.clone());
            }
        }

        UnifiedBacklogResponse {
            source: "xavier/src/server/maloca/backlog_route".to_string(),
            total: matched_items.len(),
            items: matched_items,
        }
    }

    /// Calculates summary metrics across all features.
    pub fn get_summary(&self) -> BacklogSummaryResponse {
        let projects = self.get_projects();
        let mut total_features = 0;
        let mut completed_features = 0;
        let mut total_progress_sum = 0.0;

        let mut wave_counts: BTreeMap<String, (usize, usize, f64)> = BTreeMap::new();
        let mut status_breakdown: BTreeMap<String, usize> = BTreeMap::new();

        for proj in &projects {
            for feat in &proj.features {
                total_features += 1;
                total_progress_sum += feat.progress_pct;

                let is_completed = feat.progress_pct >= 100.0
                    || feat.status.eq_ignore_ascii_case("stable")
                    || feat.status.eq_ignore_ascii_case("implemented")
                    || feat.status.eq_ignore_ascii_case("completed");

                if is_completed {
                    completed_features += 1;
                }

                *status_breakdown
                    .entry(feat.status.to_lowercase())
                    .or_insert(0) += 1;

                let wave_key = feat
                    .wave
                    .map(|w| w.to_string())
                    .unwrap_or_else(|| "unassigned".to_string());

                let entry = wave_counts.entry(wave_key).or_insert((0, 0, 0.0));
                entry.0 += 1;
                if is_completed {
                    entry.1 += 1;
                }
                entry.2 += feat.progress_pct;
            }
        }

        let overall_progress_pct = if total_features > 0 {
            (total_progress_sum / total_features as f64 * 10.0).round() / 10.0
        } else {
            0.0
        };

        let mut waves = BTreeMap::new();
        for (w_key, (w_total, w_completed, w_sum)) in wave_counts {
            let w_pct = if w_total > 0 {
                (w_sum / w_total as f64 * 10.0).round() / 10.0
            } else {
                0.0
            };
            waves.insert(
                w_key,
                WaveSummary {
                    total_features: w_total,
                    completed_features: w_completed,
                    progress_pct: w_pct,
                },
            );
        }

        BacklogSummaryResponse {
            source: "xavier/src/server/maloca/backlog_route".to_string(),
            total_features,
            completed_features,
            overall_progress_pct,
            waves,
            status_breakdown,
        }
    }
}

/// Directory names never descended into while scanning for `.gitcore/features.json`.
/// `node_modules`/`target` are huge dependency/build trees, `.git` is internal repo
/// plumbing, and `archivo/` is SWAL's explicitly-retired archive (see workspace
/// `AGENTS.md`: "Archivo — Histórico — NO reactivar").
const IGNORED_SCAN_DIR_NAMES: &[&str] = &["node_modules", "target", ".git", "archivo"];

/// Maximum recursion depth (in directory levels below `base_dir`) while scanning
/// for `.gitcore/features.json`. Real SWAL products live at
/// `apps/<app>/.gitcore/features.json` and `cores/<core>/.gitcore/features.json` —
/// two levels below the workspace root — so a flat 1-level scan (base_dir + its
/// immediate children only) silently missed every one of them.
const MAX_SCAN_DEPTH: usize = 3;

/// Helper function to scan directory (up to [`MAX_SCAN_DEPTH`] levels deep) for
/// projects containing `.gitcore/features.json`, e.g. both `<base>/.gitcore/...`
/// (standalone project root) and `<base>/apps/<app>/.gitcore/...` /
/// `<base>/cores/<core>/.gitcore/...` (SWAL workspace layout).
fn scan_projects_from_directory(base_dir: &Path) -> Vec<UnifiedProject> {
    let mut projects = Vec::new();
    let mut seen = std::collections::HashSet::new();
    scan_dir_recursive(base_dir, 0, MAX_SCAN_DEPTH, &mut projects, &mut seen);
    projects
}

fn scan_dir_recursive(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    projects: &mut Vec<UnifiedProject>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    let gitcore_features = dir.join(".gitcore").join("features.json");
    if gitcore_features.is_file() {
        let dedup_key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if seen.insert(dedup_key) {
            if let Some(proj) = parse_project_features(dir, &gitcore_features) {
                projects.push(proj);
            }
        }
    }

    if depth >= max_depth {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Skip build/VCS/dependency noise and any other hidden directory — the
        // only hidden name we ever care about is `.gitcore`, and that is read
        // directly above without needing to descend into it.
        if name.starts_with('.') || IGNORED_SCAN_DIR_NAMES.contains(&name) {
            continue;
        }
        scan_dir_recursive(&path, depth + 1, max_depth, projects, seen);
    }
}

/// Parses `.gitcore/features.json` into a `UnifiedProject`.
fn parse_project_features(proj_dir: &Path, features_json_path: &Path) -> Option<UnifiedProject> {
    let content = std::fs::read_to_string(features_json_path).ok()?;
    let json_val: serde_json::Value = serde_json::from_str(&content).ok()?;

    let repo_name = json_val
        .get("metadata")
        .and_then(|m| m.get("project"))
        .and_then(|p| p.as_str())
        .map(String::from)
        .or_else(|| {
            proj_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "unknown-app".to_string());

    let mut features = Vec::new();

    let raw_features = json_val.get("features")?;

    let feature_values: Vec<(Option<String>, &serde_json::Value)> = match raw_features {
        serde_json::Value::Array(arr) => arr.iter().map(|item| (None, item)).collect(),
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(key, val)| (Some(key.clone()), val))
            .collect(),
        _ => Vec::new(),
    };

    for (key_opt, feat_val) in feature_values {
        let id = feat_val
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or(key_opt)
            .unwrap_or_default();

        if id.is_empty() {
            continue;
        }

        let name = feat_val
            .get("name")
            .or_else(|| feat_val.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();

        let status = feat_val
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("planned")
            .to_string();

        let progress_pct = feat_val
            .get("progress_pct")
            .or_else(|| feat_val.get("progress"))
            .or_else(|| feat_val.get("percent"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let category = feat_val
            .get("category")
            .and_then(|v| v.as_str())
            .map(String::from);

        let notes = feat_val
            .get("notes")
            .and_then(|v| v.as_str())
            .map(String::from);

        let github_issue = feat_val.get("github_issue").cloned();

        // Extract wave
        let wave = feat_val
            .get("wave")
            .and_then(|v| v.as_u64().map(|w| w as u32))
            .or_else(|| parse_wave_from_str(&id))
            .or_else(|| notes.as_deref().and_then(parse_wave_from_str));

        // Extract priority
        let priority = feat_val
            .get("priority")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| notes.as_deref().and_then(parse_priority_from_str));

        features.push(UnifiedFeatureItem {
            id,
            name,
            app_id: repo_name.clone(),
            wave,
            status,
            priority,
            progress_pct,
            category,
            notes,
            github_issue,
        });
    }

    Some(UnifiedProject {
        repo_name,
        features,
    })
}

/// Helper function to parse wave integer from text (e.g., "wave 15", "wave-15", "ola 15").
fn parse_wave_from_str(s: &str) -> Option<u32> {
    let lower = s.to_lowercase();
    for token in ["wave-", "wave ", "wave_", "ola ", "ola-", "ola_"] {
        if let Some(pos) = lower.find(token) {
            let rest = &lower[pos + token.len()..];
            let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(val) = num_str.parse::<u32>() {
                return Some(val);
            }
        }
    }
    None
}

/// Helper function to parse priority (P0, P1, P2, P3) from text.
fn parse_priority_from_str(s: &str) -> Option<String> {
    let upper = s.to_uppercase();
    for p in ["P0", "P1", "P2", "P3"] {
        if upper.contains(p) {
            return Some(p.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Axum Handlers & Router
// ---------------------------------------------------------------------------

/// GET `/v1/maloca/backlog/unified`: Aggregates features from all SWAL product repos with optional filters.
pub async fn unified_backlog_handler(
    State(service): State<UnifiedBacklogService>,
    Query(query): Query<BacklogQuery>,
) -> impl IntoResponse {
    let response = service.get_unified_backlog(&query);
    Json(response)
}

/// GET `/v1/maloca/backlog/summary`: Returns total progress percentage and breakdown per wave.
pub async fn backlog_summary_handler(
    State(service): State<UnifiedBacklogService>,
) -> impl IntoResponse {
    let response = service.get_summary();
    Json(response)
}

/// Router for unified backlog aggregation endpoints.
pub fn router(service: UnifiedBacklogService) -> Router {
    Router::new()
        .route("/v1/maloca/backlog/unified", get(unified_backlog_handler))
        .route("/v1/maloca/backlog/summary", get(backlog_summary_handler))
        .with_state(service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;

    fn setup_mock_swal_workspace() -> tempfile::TempDir {
        let tmp = tempdir().unwrap();

        // Project 1: Shelf app
        let shelf_dir = tmp.path().join("shelf").join(".gitcore");
        create_dir_all(&shelf_dir).unwrap();
        let shelf_features = r#"{
            "protocol": "3.8.0",
            "metadata": { "project": "shelf" },
            "features": [
                {
                    "id": "feat-shelf-storage",
                    "name": "Shelf Storage Engine",
                    "status": "planned",
                    "progress_pct": 0.0,
                    "wave": 1,
                    "priority": "P0",
                    "notes": "Core storage for shelf"
                },
                {
                    "id": "feat-shelf-sync",
                    "name": "Shelf Sync",
                    "status": "stable",
                    "progress_pct": 100.0,
                    "wave": 1,
                    "priority": "P1"
                }
            ]
        }"#;
        write(shelf_dir.join("features.json"), shelf_features).unwrap();

        // Project 2: Xavier app
        let xavier_dir = tmp.path().join("xavier").join(".gitcore");
        create_dir_all(&xavier_dir).unwrap();
        let xavier_features = r#"{
            "protocol": "3.8.0",
            "metadata": { "project": "xavier" },
            "features": {
                "feat-xavier-backlog": {
                    "id": "feat-xavier-backlog",
                    "name": "Global Unified Backlog",
                    "status": "stable",
                    "progress_pct": 100.0,
                    "notes": "Wave 15 issue implementation P0"
                },
                "feat-xavier-mesh": {
                    "id": "feat-xavier-mesh",
                    "name": "Mesh Sync",
                    "status": "planned",
                    "progress_pct": 20.0,
                    "wave": 2,
                    "priority": "P2"
                }
            }
        }"#;
        write(xavier_dir.join("features.json"), xavier_features).unwrap();

        tmp
    }

    #[test]
    fn test_unified_backlog_aggregation_and_filters() {
        let mock_workspace = setup_mock_swal_workspace();
        let service =
            UnifiedBacklogService::new().with_workspace_dir(mock_workspace.path().to_path_buf());

        // Unfiltered query
        let all = service.get_unified_backlog(&BacklogQuery::default());
        assert_eq!(all.total, 4);

        // Filter by app_id
        let shelf_only = service.get_unified_backlog(&BacklogQuery {
            app_id: Some("shelf".to_string()),
            ..Default::default()
        });
        assert_eq!(shelf_only.total, 2);
        assert!(shelf_only.items.iter().all(|i| i.app_id == "shelf"));

        // Filter by wave
        let wave1_only = service.get_unified_backlog(&BacklogQuery {
            wave: Some(1),
            ..Default::default()
        });
        assert_eq!(wave1_only.total, 2);
        assert!(wave1_only.items.iter().all(|i| i.wave == Some(1)));

        // Filter by wave 15 (extracted from notes)
        let wave15_only = service.get_unified_backlog(&BacklogQuery {
            wave: Some(15),
            ..Default::default()
        });
        assert_eq!(wave15_only.total, 1);
        assert_eq!(wave15_only.items[0].id, "feat-xavier-backlog");

        // Filter by priority P0
        let prio_p0 = service.get_unified_backlog(&BacklogQuery {
            priority: Some("P0".to_string()),
            ..Default::default()
        });
        assert_eq!(prio_p0.total, 2);

        // Filter by status planned
        let planned = service.get_unified_backlog(&BacklogQuery {
            status: Some("planned".to_string()),
            ..Default::default()
        });
        assert_eq!(planned.total, 2);
    }

    #[test]
    fn test_backlog_summary_metrics() {
        let mock_workspace = setup_mock_swal_workspace();
        let service =
            UnifiedBacklogService::new().with_workspace_dir(mock_workspace.path().to_path_buf());

        let summary = service.get_summary();
        assert_eq!(summary.total_features, 4);
        assert_eq!(summary.completed_features, 2); // shelf-sync (100%) and xavier-backlog (stable 100%)

        // Check wave breakdown
        assert!(summary.waves.contains_key("1"));
        assert!(summary.waves.contains_key("2"));
        assert!(summary.waves.contains_key("15"));

        let w1 = summary.waves.get("1").unwrap();
        assert_eq!(w1.total_features, 2);
        assert_eq!(w1.completed_features, 1);
        assert_eq!(w1.progress_pct, 50.0);

        // Check status breakdown
        assert_eq!(*summary.status_breakdown.get("planned").unwrap(), 2);
        assert_eq!(*summary.status_breakdown.get("stable").unwrap(), 2);
    }

    #[test]
    fn test_in_memory_cache_ttl() {
        let mock_workspace = setup_mock_swal_workspace();
        let service =
            UnifiedBacklogService::new().with_workspace_dir(mock_workspace.path().to_path_buf());

        // Initial scan populates cache
        let projects1 = service.get_projects();
        assert_eq!(projects1.len(), 2);

        // Verify cache is populated
        {
            let guard = service.cache.read().unwrap();
            assert!(guard.is_some());
        }

        // Cache clear works
        service.clear_cache();
        {
            let guard = service.cache.read().unwrap();
            assert!(guard.is_none());
        }
    }

    /// Mirrors the real SWAL workspace layout audited in bug #4: products
    /// live two levels below the workspace root, at
    /// `apps/<app>/.gitcore/features.json` and `cores/<core>/.gitcore/features.json`.
    /// A flat 1-level scan (workspace root + its immediate children only)
    /// never reaches these — this test pins the depth-2/3 recursive fix.
    fn setup_nested_apps_and_cores_workspace() -> tempfile::TempDir {
        let tmp = tempdir().unwrap();

        let xavier_dir = tmp.path().join("apps").join("xavier").join(".gitcore");
        create_dir_all(&xavier_dir).unwrap();
        write(
            xavier_dir.join("features.json"),
            r#"{
                "metadata": { "project": "xavier" },
                "features": [
                    { "id": "feat-xavier-nested", "name": "Nested Xavier Feature",
                      "status": "planned", "progress_pct": 10.0 }
                ]
            }"#,
        )
        .unwrap();

        let edge_mesh_dir = tmp.path().join("cores").join("edge-mesh").join(".gitcore");
        create_dir_all(&edge_mesh_dir).unwrap();
        write(
            edge_mesh_dir.join("features.json"),
            r#"{
                "metadata": { "project": "edge-mesh" },
                "features": [
                    { "id": "feat-edge-mesh-nested", "name": "Nested Edge Mesh Feature",
                      "status": "stable", "progress_pct": 100.0 }
                ]
            }"#,
        )
        .unwrap();

        // Noise that must NOT be descended into: a features.json buried
        // inside node_modules/target/.git/archivo must never surface.
        for ignored in ["node_modules", "target", ".git", "archivo"] {
            let noise_dir = tmp
                .path()
                .join("apps")
                .join(ignored)
                .join("nested-app")
                .join(".gitcore");
            create_dir_all(&noise_dir).unwrap();
            write(
                noise_dir.join("features.json"),
                r#"{"metadata":{"project":"should-never-appear"},"features":[
                    {"id":"feat-should-be-ignored","name":"x","status":"planned","progress_pct":0.0}
                ]}"#,
            )
            .unwrap();
        }

        tmp
    }

    #[test]
    fn test_scan_reaches_apps_and_cores_two_levels_deep() {
        let mock_workspace = setup_nested_apps_and_cores_workspace();
        let service =
            UnifiedBacklogService::new().with_workspace_dir(mock_workspace.path().to_path_buf());

        let all = service.get_unified_backlog(&BacklogQuery::default());
        let ids: Vec<&str> = all.items.iter().map(|i| i.id.as_str()).collect();

        assert!(
            ids.contains(&"feat-xavier-nested"),
            "apps/xavier/.gitcore/features.json (depth 2) must be found; got {ids:?}"
        );
        assert!(
            ids.contains(&"feat-edge-mesh-nested"),
            "cores/edge-mesh/.gitcore/features.json (depth 2) must be found; got {ids:?}"
        );
        assert!(
            !ids.contains(&"feat-should-be-ignored"),
            "features.json inside node_modules/target/.git/archivo must never be scanned; got {ids:?}"
        );
        assert_eq!(
            all.total, 2,
            "only the two legitimate nested features should surface"
        );
    }

    #[test]
    fn test_unified_backlog_includes_maloca_store_backlog_items() {
        let mock_workspace = setup_nested_apps_and_cores_workspace();
        let store_dir = tempdir().unwrap();
        let maloca_store = crate::maloca::MalocaStore::open(store_dir.path());

        let service = UnifiedBacklogService::new()
            .with_workspace_dir(mock_workspace.path().to_path_buf())
            .with_maloca_store(maloca_store);

        let all = service.get_unified_backlog(&BacklogQuery::default());
        let ids: Vec<&str> = all.items.iter().map(|i| i.id.as_str()).collect();

        // Seeded by `default_state()` in store.rs: "maloca-ui" / "maloca-pwa".
        assert!(
            ids.contains(&"maloca-ui"),
            "store.backlog() items must be folded into the unified response; got {ids:?}"
        );
        assert!(ids.contains(&"maloca-pwa"));
        // Plus the two scanned .gitcore features from apps/ and cores/.
        assert_eq!(all.total, 4);
    }
}
