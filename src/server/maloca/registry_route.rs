//! Ecosystem Apps Registry REST Endpoint (`/v1/maloca/registry`).
//!
//! Provides cached access and single app entry lookup for the SWAL ecosystem app registry.
//! Supports ETag conditional GET headers (`If-None-Match`) for efficient HTTP caching.

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Arc, RwLock};

/// Information about a repository in the SWAL ecosystem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepositoryInfo {
    /// Name or URI of the repository.
    pub repo: String,
    /// Lifecycle or development state (e.g. "active", "stable", "beta").
    pub state: String,
    /// Definition of Done (DoD) score (0.0 to 100.0).
    pub dod_score: f64,
}

/// Information about licensing for an application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LicenseInfo {
    /// License human-readable name.
    pub name: String,
    /// SPDX license identifier.
    pub spdx_id: String,
    /// License URL if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Namespace mapping for routing or memory partitioning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NamespaceMapping {
    /// Namespace identifier.
    pub namespace: String,
    /// Mapping target identifier or prefix.
    pub target: String,
}

/// A single application entry in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppRegistryEntry {
    /// Unique identifier of the app (e.g., "xavier", "maloca", "code-graph", "panel-ui").
    pub app_id: String,
    /// Display name of the application.
    pub name: String,
    /// Ecosystem role of the application.
    pub role: String,
    /// Short description of the application.
    pub description: String,
    /// Repository information.
    pub repository: RepositoryInfo,
    /// Licensing information.
    pub license: LicenseInfo,
    /// Mapped namespaces.
    #[serde(default)]
    pub namespaces: Vec<NamespaceMapping>,
    /// Additional metadata fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Canonical v2 registry of SWAL applications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppRegistry {
    /// Schema version (e.g., "2.0").
    pub version: String,
    /// ISO 8601 timestamp of last registry update.
    pub updated_at: String,
    /// List of registered application entries.
    pub apps: Vec<AppRegistryEntry>,
}

impl Default for AppRegistry {
    fn default() -> Self {
        Self {
            version: "2.0".to_string(),
            updated_at: "2026-07-15T00:00:00Z".to_string(),
            apps: vec![
                AppRegistryEntry {
                    app_id: "xavier".to_string(),
                    name: "Xavier Core Runtime".to_string(),
                    role: "Cognitive Memory Runtime & MCP Server".to_string(),
                    description: "High-performance vector memory engine and AI agent coordinator."
                        .to_string(),
                    repository: RepositoryInfo {
                        repo: "iberi22/xavier".to_string(),
                        state: "active".to_string(),
                        dod_score: 95.5,
                    },
                    license: LicenseInfo {
                        name: "GNU Affero General Public License v3.0".to_string(),
                        spdx_id: "AGPL-3.0-only".to_string(),
                        url: Some("https://www.gnu.org/licenses/agpl-3.0.html".to_string()),
                    },
                    namespaces: vec![NamespaceMapping {
                        namespace: "xavier".to_string(),
                        target: "memory/v1".to_string(),
                    }],
                    metadata: None,
                },
                AppRegistryEntry {
                    app_id: "maloca".to_string(),
                    name: "Maloca Portal".to_string(),
                    role: "Decentralized P2P Data Commons & Identity Network".to_string(),
                    description: "Social consensus and memory replication portal for SWAL nodes."
                        .to_string(),
                    repository: RepositoryInfo {
                        repo: "iberi22/swal-apps-registry".to_string(),
                        state: "active".to_string(),
                        dod_score: 92.0,
                    },
                    license: LicenseInfo {
                        name: "GNU Affero General Public License v3.0".to_string(),
                        spdx_id: "AGPL-3.0-only".to_string(),
                        url: Some("https://www.gnu.org/licenses/agpl-3.0.html".to_string()),
                    },
                    namespaces: vec![NamespaceMapping {
                        namespace: "maloca".to_string(),
                        target: "p2p/mesh".to_string(),
                    }],
                    metadata: None,
                },
                AppRegistryEntry {
                    app_id: "code-graph".to_string(),
                    name: "Code Graph Engine".to_string(),
                    role: "AST Symbol Analysis & Blast Radius Engine".to_string(),
                    description: "Language server AST indexer and dependency graph analyzer."
                        .to_string(),
                    repository: RepositoryInfo {
                        repo: "iberi22/xavier/code-graph".to_string(),
                        state: "active".to_string(),
                        dod_score: 90.0,
                    },
                    license: LicenseInfo {
                        name: "GNU Affero General Public License v3.0".to_string(),
                        spdx_id: "AGPL-3.0-only".to_string(),
                        url: Some("https://www.gnu.org/licenses/agpl-3.0.html".to_string()),
                    },
                    namespaces: vec![NamespaceMapping {
                        namespace: "code-graph".to_string(),
                        target: "symbols/v1".to_string(),
                    }],
                    metadata: None,
                },
                AppRegistryEntry {
                    app_id: "panel-ui".to_string(),
                    name: "Panel UI Frontend".to_string(),
                    role: "Management Dashboard & Visualizer".to_string(),
                    description:
                        "React and Tauri frontend UI for Xavier memory and Maloca control."
                            .to_string(),
                    repository: RepositoryInfo {
                        repo: "iberi22/xavier/panel-ui".to_string(),
                        state: "active".to_string(),
                        dod_score: 88.5,
                    },
                    license: LicenseInfo {
                        name: "GNU Affero General Public License v3.0".to_string(),
                        spdx_id: "AGPL-3.0-only".to_string(),
                        url: Some("https://www.gnu.org/licenses/agpl-3.0.html".to_string()),
                    },
                    namespaces: vec![NamespaceMapping {
                        namespace: "panel".to_string(),
                        target: "ui/v1".to_string(),
                    }],
                    metadata: None,
                },
            ],
        }
    }
}

/// Cached state of the app registry and ETag calculation.
#[derive(Debug, Clone)]
pub struct CachedRegistry {
    pub registry: AppRegistry,
    pub raw_json: String,
    pub etag: String,
}

impl CachedRegistry {
    /// Creates a cached registry from an `AppRegistry` struct.
    pub fn new(registry: AppRegistry) -> Self {
        let raw_json = serde_json::to_string(&registry).unwrap_or_default();
        let etag = Self::calculate_etag(raw_json.as_bytes());
        Self {
            registry,
            raw_json,
            etag,
        }
    }

    /// Calculates SHA-256 ETag formatted header value.
    pub fn calculate_etag(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let hash = hasher.finalize();
        format!("\"{}\"", crate::crypto::hex_encode(hash))
    }
}

/// In-memory manager for loading, serving, and caching SWAL app registry.
#[derive(Clone, Debug)]
pub struct AppRegistryManager {
    cache: Arc<RwLock<CachedRegistry>>,
    file_path: PathBuf,
}

impl Default for AppRegistryManager {
    fn default() -> Self {
        Self::with_path("apps/maloca/packages/swal-apps-registry/src/registry.json")
    }
}

impl AppRegistryManager {
    /// Constructs a manager initialized with the specified file path or canonical fallback.
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        let file_path = path.into();
        let cache = Arc::new(RwLock::new(Self::load_or_fallback(&file_path)));
        Self { cache, file_path }
    }

    /// Attempts to load registry from file; falls back to default if file missing or unparseable.
    fn load_or_fallback(path: &StdPath) -> CachedRegistry {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(registry) = serde_json::from_str::<AppRegistry>(&content) {
                    return CachedRegistry::new(registry);
                }
            }
        }
        CachedRegistry::new(AppRegistry::default())
    }

    /// Forces a refresh from disk.
    pub fn reload(&self) {
        let updated = Self::load_or_fallback(&self.file_path);
        if let Ok(mut guard) = self.cache.write() {
            *guard = updated;
        }
    }

    /// Gets current cached registry object.
    pub fn get_cached(&self) -> CachedRegistry {
        self.cache
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| CachedRegistry::new(AppRegistry::default()))
    }

    /// Looks up a single app entry by `app_id`.
    pub fn get_app(&self, app_id: &str) -> Option<AppRegistryEntry> {
        let cached = self.get_cached();
        cached
            .registry
            .apps
            .into_iter()
            .find(|app| app.app_id.eq_ignore_ascii_case(app_id))
    }
}

/// Response payload for error conditions.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistryErrorResponse {
    pub error: String,
}

/// GET `/v1/maloca/registry`: Full JSON registry of SWAL repositories, roles, licenses, and namespace mappings.
pub async fn get_registry_handler(
    State(manager): State<AppRegistryManager>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let cached = manager.get_cached();
    let etag = cached.etag;

    // Check If-None-Match header for ETag matching
    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH) {
        if let Ok(req_etag) = if_none_match.to_str() {
            if req_etag.trim() == etag {
                return (
                    StatusCode::NOT_MODIFIED,
                    [(header::ETAG, etag)],
                    String::new(),
                )
                    .into_response();
            }
        }
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::ETAG, etag.as_str()),
        ],
        cached.raw_json,
    )
        .into_response()
}

/// GET `/v1/maloca/registry/{app_id}`: Single app registry entry lookup.
pub async fn get_app_entry_handler(
    State(manager): State<AppRegistryManager>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    match manager.get_app(&app_id) {
        Some(entry) => (StatusCode::OK, Json(entry)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(RegistryErrorResponse {
                error: format!("App entry with ID '{}' not found", app_id),
            }),
        )
            .into_response(),
    }
}

/// Constructs the Axum router for the Ecosystem Apps Registry endpoints.
pub fn router(manager: AppRegistryManager) -> Router {
    Router::new()
        .route("/v1/maloca/registry", get(get_registry_handler))
        .route("/v1/maloca/registry/{app_id}", get(get_app_entry_handler))
        .with_state(manager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tempfile::NamedTempFile;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_get_full_registry_default_and_etag() {
        let manager = AppRegistryManager::with_path("non_existent_path.json");
        let app = router(manager.clone());

        // 1. Initial GET request
        let req = Request::builder()
            .uri("/v1/maloca/registry")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let etag_header = response
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(!etag_header.is_empty());

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let registry: AppRegistry = serde_json::from_slice(&body).unwrap();
        assert_eq!(registry.version, "2.0");
        assert!(registry.apps.iter().any(|a| a.app_id == "xavier"));

        // 2. Conditional GET with matching If-None-Match
        let cond_req = Request::builder()
            .uri("/v1/maloca/registry")
            .header(header::IF_NONE_MATCH, etag_header.clone())
            .body(axum::body::Body::empty())
            .unwrap();

        let cond_response = app.oneshot(cond_req).await.unwrap();
        assert_eq!(cond_response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn test_get_app_entry_found_and_not_found() {
        let manager = AppRegistryManager::with_path("non_existent_path.json");
        let app = router(manager);

        // Found app entry
        let req = Request::builder()
            .uri("/v1/maloca/registry/xavier")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let entry: AppRegistryEntry = serde_json::from_slice(&body).unwrap();
        assert_eq!(entry.app_id, "xavier");
        assert_eq!(entry.repository.repo, "iberi22/xavier");
        assert_eq!(entry.license.spdx_id, "AGPL-3.0-only");

        // Not found app entry
        let req_missing = Request::builder()
            .uri("/v1/maloca/registry/unknown-app-xyz")
            .body(axum::body::Body::empty())
            .unwrap();

        let response_missing = app.oneshot(req_missing).await.unwrap();
        assert_eq!(response_missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_load_custom_file() {
        let custom_registry = AppRegistry {
            version: "2.0".to_string(),
            updated_at: "2026-07-15T12:00:00Z".to_string(),
            apps: vec![AppRegistryEntry {
                app_id: "test-app".to_string(),
                name: "Test App".to_string(),
                role: "Testing".to_string(),
                description: "Test description".to_string(),
                repository: RepositoryInfo {
                    repo: "org/test-app".to_string(),
                    state: "beta".to_string(),
                    dod_score: 100.0,
                },
                license: LicenseInfo {
                    name: "AGPL-3.0".to_string(),
                    spdx_id: "AGPL-3.0-only".to_string(),
                    url: None,
                },
                namespaces: vec![],
                metadata: None,
            }],
        };

        let temp_file = NamedTempFile::new().unwrap();
        let json_data = serde_json::to_string(&custom_registry).unwrap();
        fs::write(temp_file.path(), json_data).unwrap();

        let manager = AppRegistryManager::with_path(temp_file.path());
        let app = router(manager);

        let req = Request::builder()
            .uri("/v1/maloca/registry/test-app")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let entry: AppRegistryEntry = serde_json::from_slice(&body).unwrap();
        assert_eq!(entry.app_id, "test-app");
        assert_eq!(entry.repository.dod_score, 100.0);
    }
}
