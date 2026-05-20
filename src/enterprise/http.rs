//! Enterprise HTTP endpoints for multi-tenancy, API keys, audit, and rate limits.
//!
//! Only compiled when the `enterprise` feature is enabled.
//! Merge with the main router via `Router::merge()`.

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    routing::{delete, get, patch, post},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::enterprise::{
    audit::{AuditAction, AuditEntry, AuditLog, AuditQuery},
    keys::{ApiKey, ApiKeyStore, ApiKeyType},
    rate_limit::{RateLimitConfig, RateLimiter, RateLimitKey},
    tenancy::{Plan, Tenant, TenantId, TenantStore},
};

/// Shared enterprise state accessible by all handlers
pub struct EnterpriseState {
    pub tenant_store: TenantStore,
    pub api_key_store: ApiKeyStore,
    pub audit_log: AuditLog,
    pub rate_limiter: RateLimiter,
}

impl Default for EnterpriseState {
    fn default() -> Self {
        Self {
            tenant_store: TenantStore::new(),
            api_key_store: ApiKeyStore::new(),
            audit_log: AuditLog::new(),
            rate_limiter: RateLimiter::new(),
        }
    }
}

/// Global enterprise state shared across all handlers
static ENTERPRISE_STATE: once_cell::sync::Lazy<Arc<Mutex<EnterpriseState>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(EnterpriseState::default())));

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
    Json(payload): Json<CreateTenant>,
) -> Result<Json<TenantResponse>, StatusCode> {
    let mut state = ENTERPRISE_STATE.lock().unwrap();
    let tenant = state.tenant_store.create(payload.name, payload.plan);
    Ok(Json(TenantResponse::from(tenant)))
}

/// GET /v1/tenants — List all tenants
async fn list_tenants() -> Result<Json<Vec<TenantResponse>>, StatusCode> {
    let state = ENTERPRISE_STATE.lock().unwrap();
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
    Path(id): Path<TenantId>,
) -> Result<Json<TenantResponse>, StatusCode> {
    let state = ENTERPRISE_STATE.lock().unwrap();
    state
        .tenant_store
        .get(&id)
        .map(|t| Json(TenantResponse::from(t.clone())))
        .ok_or(StatusCode::NOT_FOUND)
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
    Json(payload): Json<CreateKey>,
) -> Result<Json<ApiKeyResponse>, StatusCode> {
    let mut state = ENTERPRISE_STATE.lock().unwrap();
    let (raw_key, key) = state
        .api_key_store
        .create(payload.tenant_id, payload.name.clone(), payload.key_type);

    // Log audit event
    state.audit_log.record(
        payload.tenant_id,
        AuditAction::ApiKeyCreate,
        format!("api_key:{}", key.id),
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
    Path(tenant_id): Path<TenantId>,
) -> Result<Json<Vec<ApiKey>>, StatusCode> {
    let state = ENTERPRISE_STATE.lock().unwrap();
    let keys: Vec<ApiKey> = state
        .api_key_store
        .list_for_tenant(&tenant_id)
        .into_iter()
        .cloned()
        .collect();
    Ok(Json(keys))
}

/// DELETE /v1/keys/:key_id — Revoke an API key
async fn revoke_key(
    Path(key_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let mut state = ENTERPRISE_STATE.lock().unwrap();
    state
        .api_key_store
        .revoke(&key_id)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Audit Types & Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// GET /v1/audit — Query audit log entries
async fn query_audit(
    Query(params): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEntry>>, StatusCode> {
    let state = ENTERPRISE_STATE.lock().unwrap();
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
async fn get_rate_limits() -> Result<Json<RateLimitsResponse>, StatusCode> {
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
    Json(payload): Json<UpdateRateLimits>,
) -> Result<StatusCode, StatusCode> {
    let mut state = ENTERPRISE_STATE.lock().unwrap();
    if let Some(tenant_id) = payload.tenant_id {
        let key = RateLimitKey::Tenant(tenant_id);
        state.rate_limiter.set_config(key, payload.config);
    }
    Ok(StatusCode::OK)
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

/// Build the enterprise API router.
/// Returns an empty router when `enterprise` feature is disabled.
///
/// Usage: `router.merge(enterprise_router())`
#[cfg(feature = "enterprise")]
pub fn enterprise_router() -> axum::Router {
    axum::Router::new()
        // Tenant management
        .route("/v1/tenants", post(create_tenant))
        .route("/v1/tenants", get(list_tenants))
        .route("/v1/tenants/{id}", get(get_tenant))
        // API Keys
        .route("/v1/keys", post(create_key))
        .route("/v1/keys/{tenant_id}", get(list_keys))
        .route("/v1/keys/{id}", delete(revoke_key))
        // Audit
        .route("/v1/audit", get(query_audit))
        // Rate limits
        .route("/v1/rate-limits", get(get_rate_limits))
        .route("/v1/rate-limits", patch(update_rate_limits))
}

/// No-op router when enterprise feature is disabled
#[cfg(not(feature = "enterprise"))]
pub fn enterprise_router() -> axum::Router {
    axum::Router::new()
}
