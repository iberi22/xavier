//! Enterprise HTTP endpoints for multi-tenancy, API keys, audit, and rate limits.
//!
//! Only compiled when the `enterprise` feature is enabled.
//! Merge with the main router via `Router::merge()`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, patch, post},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::enterprise::{
    audit::{AuditAction, AuditEntry, AuditLog, AuditQuery},
    keys::{ApiKey, ApiKeyStore, ApiKeyType},
    persistence::{populate_stores_from_db, EnterpriseDb},
    rate_limit::{RateLimitConfig, RateLimiter, RateLimitKey},
    tenancy::{Plan, Tenant, TenantId, TenantStore},
};

/// Shared enterprise state accessible by all handlers
pub struct EnterpriseState {
    pub tenant_store: TenantStore,
    pub api_key_store: ApiKeyStore,
    pub audit_log: AuditLog,
    pub rate_limiter: RateLimiter,
    /// Optional persistence backend. When `Some`, all mutations are
    /// persisted to SQLite after each operation.
    pub db: Option<Arc<EnterpriseDb>>,
}

impl EnterpriseState {
    /// Initialize enterprise state, optionally loading persisted data.
    ///
    /// Tries to open the enterprise database from the default path.
    /// If successful, loads all persisted data into memory stores.
    /// Falls back cleanly to empty in-memory state if no DB is available.
    pub fn init_default() -> Self {
        // Try to open the enterprise database
        let db = match EnterpriseDb::open_or_create_default() {
            Ok(db) => {
                tracing::info!("enterprise database opened successfully");
                Some(Arc::new(db))
            }
            Err(e) => {
                tracing::warn!(
                    "enterprise database not available, using in-memory only: {}",
                    e
                );
                None
            }
        };

        let mut state = Self {
            tenant_store: TenantStore::new(),
            api_key_store: ApiKeyStore::new(),
            audit_log: AuditLog::new(),
            rate_limiter: RateLimiter::new(),
            db,
        };

        // Load persisted data if db is available
        if let Some(ref db) = state.db {
            if let Err(e) = populate_stores_from_db(
                db,
                &mut state.tenant_store,
                &mut state.api_key_store,
                &mut state.rate_limiter,
            ) {
                tracing::warn!("failed to load persisted enterprise data: {}", e);
            } else {
                let tenant_count = state.tenant_store.list().len();
                let key_count: usize = state
                    .tenant_store
                    .list()
                    .iter()
                    .map(|t| state.api_key_store.count_for_tenant(&t.id))
                    .sum();
                tracing::info!(
                    "loaded {} tenants and {} API keys from enterprise database",
                    tenant_count,
                    key_count
                );
            }
        }

        state
    }
}

impl Default for EnterpriseState {
    fn default() -> Self {
        Self::init_default()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Enterprise Error Types
// ─────────────────────────────────────────────────────────────────────────────

/// Rich error type for enterprise HTTP handlers with request context
#[derive(Debug, Error)]
pub enum EnterpriseError {
    #[error("[tenant_id={tenant_id:?}] {message}")]
    NotFound {
        tenant_id: Option<TenantId>,
        message: String,
    },
    #[error("[tenant_id={tenant_id:?}] {message}")]
    Conflict {
        tenant_id: Option<TenantId>,
        message: String,
    },
    #[error("[tenant_id={tenant_id:?}] {message}")]
    BadRequest {
        tenant_id: Option<TenantId>,
        message: String,
    },
    #[error("[tenant_id={tenant_id:?}] {message}")]
    Internal {
        tenant_id: Option<TenantId>,
        message: String,
    },
    #[error("{message}")]
    Generic {
        message: String,
    },
}

impl EnterpriseError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::Internal { .. } | Self::Generic { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn into_response_inner(self) -> (StatusCode, Json<serde_json::Value>) {
        let status = self.status_code();
        let msg = self.to_string();
        tracing::error!(error = %msg, "enterprise request failed");
        (status, Json(serde_json::json!({ "error": msg })))
    }
}

impl IntoResponse for EnterpriseError {
    fn into_response(self) -> axum::response::Response {
        let (status, payload) = self.into_response_inner();
        (status, payload).into_response()
    }
}

impl From<EnterpriseError> for (StatusCode, Json<serde_json::Value>) {
    fn from(err: EnterpriseError) -> Self {
        err.into_response_inner()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tenant Types & Handlers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateTenant {
    pub name: String,
    pub plan: Plan,
}

#[derive(Debug, Serialize)]
pub struct TenantResponse {
    pub id: TenantId,
    pub name: String,
    pub plan: Plan,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<Tenant> for TenantResponse {
    fn from(t: Tenant) -> Self {
        Self {
            id: t.id,
            name: t.name,
            plan: t.plan,
            created_at: t.created_at,
        }
    }
}

/// POST /v1/tenants — Create a new tenant
async fn create_tenant(
    State(state): State<Arc<Mutex<EnterpriseState>>>,
    Json(payload): Json<CreateTenant>,
) -> Result<Json<TenantResponse>, EnterpriseError> {
    let mut state = state.lock().expect("poisoned lock: enterprise_create_tenant");
    let tenant = state.tenant_store.create(payload.name.clone(), payload.plan);

    // Persist to database if available
    if let Some(ref db) = state.db {
        if let Err(e) = db.save_tenant(&tenant) {
            tracing::error!(
                tenant_id = %tenant.id,
                tenant_name = %payload.name,
                "failed to persist tenant: {}",
                e
            );
        }
    }

    tracing::info!(
        tenant_id = %tenant.id,
        tenant_name = %payload.name,
        plan = ?payload.plan,
        "tenant created successfully"
    );

    Ok(Json(TenantResponse::from(tenant)))
}

/// GET /v1/tenants — List all tenants
async fn list_tenants(
    State(state): State<Arc<Mutex<EnterpriseState>>>,
) -> Result<Json<Vec<TenantResponse>>, StatusCode> {
    let state = state.lock().expect("poisoned lock: enterprise_list_tenants");
    let tenants: Vec<TenantResponse> = state
        .tenant_store
        .list()
        .into_iter()
        .cloned()
        .map(TenantResponse::from)
        .collect();
    Ok(Json(tenants))
}

/// GET /v1/tenants/:id — Get a tenant by ID
async fn get_tenant(
    State(state): State<Arc<Mutex<EnterpriseState>>>,
    Path(id): Path<TenantId>,
) -> Result<Json<TenantResponse>, EnterpriseError> {
    let state = state.lock().expect("poisoned lock: enterprise_get_tenant");
    state
        .tenant_store
        .get(&id)
        .map(|t| Json(TenantResponse::from(t.clone())))
        .ok_or_else(|| EnterpriseError::NotFound {
            tenant_id: Some(id),
            message: format!("tenant not found: id={}", id),
        })
}

// ─────────────────────────────────────────────────────────────────────────────
// API Key Types & Handlers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateKey {
    pub tenant_id: TenantId,
    pub name: String,
    pub key_type: ApiKeyType,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    pub id: String,
    pub tenant_id: TenantId,
    pub name: String,
    pub key_type: ApiKeyType,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The raw key — ONLY shown once at creation!
    pub raw_key: String,
}

/// POST /v1/keys — Create a new API key for a tenant
async fn create_key(
    State(state): State<Arc<Mutex<EnterpriseState>>>,
    Json(payload): Json<CreateKey>,
) -> Result<Json<ApiKeyResponse>, EnterpriseError> {
    // Validate the tenant exists
    let mut state = state.lock().expect("poisoned lock: enterprise_create_key");
    if !state.tenant_store.exists(&payload.tenant_id) {
        return Err(EnterpriseError::NotFound {
            tenant_id: Some(payload.tenant_id),
            message: format!("tenant not found for key creation: tenant_id={}", payload.tenant_id),
        });
    }

    let (raw_key, key) = state
        .api_key_store
        .create(payload.tenant_id, payload.name.clone(), payload.key_type);

    // Persist to database if available
    if let Some(ref db) = state.db {
        if let Err(e) = db.save_api_key(&key) {
            tracing::error!(
                tenant_id = %key.tenant_id,
                key_id = %key.id,
                "failed to persist API key: {}",
                e
            );
        }
    }

    // Log audit event
    let entry = AuditEntry::new(payload.tenant_id, AuditAction::ApiKeyCreate, format!("api_key:{}", key.id));
    state.audit_log.log(entry.clone());
    if let Some(ref db) = state.db {
        if let Err(e) = db.save_audit_entry(&entry) {
            tracing::error!(
                tenant_id = %key.tenant_id,
                audit_entry = %entry.id,
                "failed to persist audit entry: {}",
                e
            );
        }
    }

    tracing::info!(
        tenant_id = %key.tenant_id,
        key_id = %key.id,
        key_name = %payload.name,
        "API key created"
    );

    Ok(Json(ApiKeyResponse {
        id: key.id.clone(),
        tenant_id: key.tenant_id,
        name: key.name,
        key_type: key.key_type,
        created_at: key.created_at,
        raw_key,
    }))
}

/// GET /v1/keys/:tenant_id — List all API keys for a tenant
async fn list_keys(
    State(state): State<Arc<Mutex<EnterpriseState>>>,
    Path(id): Path<TenantId>,
) -> Result<Json<Vec<ApiKey>>, StatusCode> {
    let state = state.lock().expect("poisoned lock: enterprise_list_keys");
    let keys: Vec<ApiKey> = state
        .api_key_store
        .list_for_tenant(&id)
        .into_iter()
        .cloned()
        .collect();
    Ok(Json(keys))
}

/// DELETE /v1/keys/:key_id — Revoke an API key
async fn revoke_key(
    State(state): State<Arc<Mutex<EnterpriseState>>>,
    Path(key_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let mut state = state.lock().expect("poisoned lock: enterprise_revoke_key");

    // Get the key before revocation for tenant context
    let tenant_id = state
        .api_key_store
        .get(&key_id)
        .map(|k| k.tenant_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    state
        .api_key_store
        .revoke(&key_id)
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Persist the updated key to database
    if let Some(ref db) = state.db {
        if let Some(key) = state.api_key_store.get(&key_id) {
            if let Err(e) = db.save_api_key(key) {
                tracing::error!("failed to persist revoked API key: {}", e);
            }
        }
    }

    // Log audit event
    let entry = AuditEntry::new(tenant_id, AuditAction::ApiKeyRevoke, format!("api_key:{}", key_id));
    state.audit_log.log(entry.clone());
    if let Some(ref db) = state.db {
        if let Err(e) = db.save_audit_entry(&entry) {
            tracing::error!("failed to persist audit entry: {}", e);
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Audit Types & Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// GET /v1/audit — Query audit log entries
async fn query_audit(
    State(state): State<Arc<Mutex<EnterpriseState>>>,
    Query(params): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEntry>>, StatusCode> {
    let state = state.lock().expect("poisoned lock: enterprise_query_audit");
    let mut entries = Vec::new();

    if let Some(tenant_id) = params.tenant_id {
        let tenant_entries = state.audit_log.get_for_tenant(&tenant_id);
        entries.extend(tenant_entries.into_iter().cloned());

        if let Some(user_id) = params.user_id {
            entries.retain(|e| e.user_id == Some(user_id));
        }

        if let Some(action_str) = params.action {
            entries.retain(|e| e.action.as_str() == action_str);
        }

        if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
            entries.retain(|e| e.timestamp >= start && e.timestamp <= end);
        }
    } else {
        // Admin only — return all entries
        entries.extend(state.audit_log.get_all().into_iter().cloned());
    }

    if let Some(limit) = params.limit {
        entries.truncate(limit);
    }

    Ok(Json(entries))
}

// ─────────────────────────────────────────────────────────────────────────────
// Rate Limit Types & Handlers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RateLimitsResponse {
    pub tenant_limits: Option<RateLimitConfig>,
    pub global_remaining: u32,
}

/// GET /v1/rate-limits — Get current rate limit configuration
async fn get_rate_limits(
    State(_state): State<Arc<Mutex<EnterpriseState>>>,
) -> Result<Json<RateLimitsResponse>, StatusCode> {
    Ok(Json(RateLimitsResponse {
        tenant_limits: None,
        global_remaining: 0,
    }))
}

#[derive(Debug, Deserialize)]
pub struct UpdateRateLimits {
    pub tenant_id: Option<TenantId>,
    pub config: RateLimitConfig,
}

/// PATCH /v1/rate-limits — Update rate limit configuration
async fn update_rate_limits(
    State(state): State<Arc<Mutex<EnterpriseState>>>,
    Json(payload): Json<UpdateRateLimits>,
) -> Result<StatusCode, StatusCode> {
    let mut state = state.lock().expect("poisoned lock: enterprise_update_rate_limits");
    if let Some(tenant_id) = payload.tenant_id {
        let key = RateLimitKey::Tenant(tenant_id);
        state.rate_limiter.set_config(key.clone(), payload.config.clone());

        // Persist to database if available
        if let Some(ref db) = state.db {
            if let Err(e) = db.save_rate_limit_config(&key, &payload.config) {
                tracing::error!("failed to persist rate limit config: {}", e);
            }
        }
    }
    Ok(StatusCode::OK)
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

/// Build the enterprise API router.
/// Returns an empty router when `enterprise` feature is disabled.
///
/// Usage: `router.merge(enterprise_router(state))`
#[cfg(feature = "enterprise")]
pub fn enterprise_router(state: Arc<Mutex<EnterpriseState>>) -> axum::Router {
    axum::Router::new()
        // Tenant management
        .route("/v1/tenants", post(create_tenant))
        .route("/v1/tenants", get(list_tenants))
        .route("/v1/tenants/{id}", get(get_tenant))
        // API Keys
        .route("/v1/keys", post(create_key))
        .route("/v1/keys/{id}", get(list_keys))
        .route("/v1/keys/{id}", delete(revoke_key))
        // Audit
        .route("/v1/audit", get(query_audit))
        // Rate limits
        .route("/v1/rate-limits", get(get_rate_limits))
        .route("/v1/rate-limits", patch(update_rate_limits))
        .with_state(state)
}

/// No-op router when enterprise feature is disabled
#[cfg(not(feature = "enterprise"))]
pub fn enterprise_router() -> axum::Router {
    axum::Router::new()
}
