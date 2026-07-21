// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! SQLite persistence for enterprise module.
//!
//! Provides durable storage for tenants, API keys, audit logs, and rate limit configs.
//! Used when the `enterprise` feature is enabled and a database path is configured.
//!
//! Data is stored at `data/enterprise.db` by default, configurable via
//! the `XAVIER_ENTERPRISE_DB_PATH` environment variable.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

use crate::enterprise::{
    audit::{AuditAction, AuditEntry},
    keys::{ApiKey, ApiKeyType},
    rate_limit::{RateLimitConfig, RateLimitKey},
    tenant::{Plan, Tenant, TenantId},
};

/// Enterprise database for persistent storage of enterprise state.
///
/// Wraps a SQLite connection and provides CRUD operations for all
/// enterprise entities (tenants, API keys, audit logs, rate limits).
///
/// The inner connection is behind a `Mutex` to provide `Sync` for
/// shared access across threads.
pub struct EnterpriseDb {
    conn: Mutex<Connection>,
}

impl EnterpriseDb {
    /// Open or create the enterprise database at the given path.
    ///
    /// Creates the parent directory if it doesn't exist, sets WAL mode,
    /// and initializes all required tables.
    pub fn open_or_create(path: &PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("failed to open enterprise database at {}", path.display()))?;

        // Enable WAL mode for better concurrent read/write performance
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )
        .context("failed to set pragmas on enterprise database")?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_tables()?;
        Ok(db)
    }

    /// Open or create with default path resolution.
    ///
    /// Checks `XAVIER_ENTERPRISE_DB_PATH` env var first, then falls back
    /// to `data/enterprise.db` relative to the current working directory.
    pub fn open_or_create_default() -> Result<Self> {
        let path = std::env::var("XAVIER_ENTERPRISE_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/enterprise.db"));
        Self::open_or_create(&path)
    }

    /// Initialize all enterprise tables if they don't already exist.
    fn init_tables(&self) -> Result<()> {
        let conn = self.conn.lock().expect("poisoned lock in init_tables");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS enterprise_tenants (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                plan TEXT NOT NULL,
                created_at TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}'
            );

            CREATE TABLE IF NOT EXISTS enterprise_api_keys (
                id TEXT PRIMARY KEY,
                hash TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                name TEXT NOT NULL,
                key_type TEXT NOT NULL,
                rate_limit INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                last_used TEXT,
                expires_at TEXT,
                revoked INTEGER NOT NULL DEFAULT 0,
                metadata TEXT NOT NULL DEFAULT '{}'
            );

            CREATE TABLE IF NOT EXISTS enterprise_audit_log (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                user_id TEXT,
                action TEXT NOT NULL,
                resource TEXT NOT NULL,
                resource_id TEXT,
                success INTEGER NOT NULL DEFAULT 1,
                details TEXT,
                ip_address TEXT,
                user_agent TEXT
            );

            CREATE TABLE IF NOT EXISTS enterprise_rate_limits (
                key_type TEXT NOT NULL,
                key_value TEXT NOT NULL,
                rpm INTEGER NOT NULL,
                burst INTEGER NOT NULL,
                PRIMARY KEY (key_type, key_value)
            );

            -- Indexes for common queries
            CREATE INDEX IF NOT EXISTS idx_api_keys_tenant ON enterprise_api_keys(tenant_id);
            CREATE INDEX IF NOT EXISTS idx_audit_tenant ON enterprise_audit_log(tenant_id);
            CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON enterprise_audit_log(timestamp);
            CREATE INDEX IF NOT EXISTS idx_audit_action ON enterprise_audit_log(action);",
        )
        .context("failed to create enterprise tables")?;
        Ok(())
    }

    // ─── Tenant CRUD ───────────────────────────────────────────────────────

    /// Insert or update a tenant in the database.
    pub fn save_tenant(&self, tenant: &Tenant) -> Result<()> {
        let metadata_json = serde_json::to_string(&tenant.settings)
            .context("failed to serialize tenant metadata")?;
        let plan_str = plan_to_string(&tenant.plan);
        let conn = self.conn.lock().expect("poisoned lock in save_tenant");
        conn.execute(
            "INSERT INTO enterprise_tenants (id, name, plan, created_at, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                plan = excluded.plan,
                metadata = excluded.metadata",
            params![
                tenant.id.to_string(),
                tenant.name,
                plan_str,
                tenant.created_at.to_rfc3339(),
                metadata_json,
            ],
        )?;
        Ok(())
    }

    /// Load all tenants from the database into a HashMap keyed by TenantId.
    pub fn load_all_tenants(&self) -> Result<HashMap<TenantId, Tenant>> {
        let conn = self.conn.lock().expect("poisoned lock in load_all_tenants");
        let mut stmt =
            conn.prepare("SELECT id, name, plan, created_at, metadata FROM enterprise_tenants")?;
        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let name: String = row.get(1)?;
            let plan_str: String = row.get(2)?;
            let created_at_str: String = row.get(3)?;
            let metadata_str: String = row.get(4)?;
            Ok((id_str, name, plan_str, created_at_str, metadata_str))
        })?;

        let mut tenants = HashMap::new();
        for row in rows {
            let (id_str, name, plan_str, created_at_str, metadata_str) = row?;
            let id: TenantId = id_str.parse()?;
            let plan: Plan = string_to_plan(&plan_str);
            let created_at: DateTime<Utc> = created_at_str.parse()?;
            let metadata: HashMap<String, String> =
                serde_json::from_str(&metadata_str).unwrap_or_default();
            tenants.insert(
                id,
                Tenant {
                    id,
                    name,
                    plan,
                    created_at,
                    settings: metadata,
                },
            );
        }
        Ok(tenants)
    }

    /// Delete a tenant by ID.
    pub fn delete_tenant(&self, id: &TenantId) -> Result<()> {
        let conn = self.conn.lock().expect("poisoned lock in delete_tenant");
        conn.execute(
            "DELETE FROM enterprise_tenants WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // ─── API Key CRUD ─────────────────────────────────────────────────────

    /// Insert or update an API key in the database.
    pub fn save_api_key(&self, key: &ApiKey) -> Result<()> {
        let metadata_json =
            serde_json::to_string(&key.metadata).context("failed to serialize API key metadata")?;
        let key_type_str = api_key_type_to_string(&key.key_type);
        let conn = self.conn.lock().expect("poisoned lock in save_api_key");
        conn.execute(
            "INSERT INTO enterprise_api_keys
                (id, hash, tenant_id, name, key_type, rate_limit,
                 created_at, last_used, expires_at, revoked, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                hash = excluded.hash,
                tenant_id = excluded.tenant_id,
                name = excluded.name,
                key_type = excluded.key_type,
                rate_limit = excluded.rate_limit,
                last_used = excluded.last_used,
                expires_at = excluded.expires_at,
                revoked = excluded.revoked,
                metadata = excluded.metadata",
            params![
                key.id,
                key.hash,
                key.tenant_id.to_string(),
                key.name,
                key_type_str,
                key.rate_limit as i64,
                key.created_at.to_rfc3339(),
                key.last_used.map(|d| d.to_rfc3339()),
                key.expires_at.map(|d| d.to_rfc3339()),
                key.revoked as i64,
                metadata_json,
            ],
        )?;
        Ok(())
    }

    /// Load all API keys from the database.
    pub fn load_all_api_keys(&self) -> Result<Vec<ApiKey>> {
        let conn = self
            .conn
            .lock()
            .expect("poisoned lock in load_all_api_keys");
        let mut stmt = conn.prepare(
            "SELECT id, hash, tenant_id, name, key_type, rate_limit,
                    created_at, last_used, expires_at, revoked, metadata
             FROM enterprise_api_keys",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let hash: String = row.get(1)?;
            let tenant_id_str: String = row.get(2)?;
            let name: String = row.get(3)?;
            let key_type_str: String = row.get(4)?;
            let rate_limit: i64 = row.get(5)?;
            let created_at_str: String = row.get(6)?;
            let last_used_str: Option<String> = row.get(7)?;
            let expires_at_str: Option<String> = row.get(8)?;
            let revoked: i64 = row.get(9)?;
            let metadata_str: String = row.get(10)?;
            Ok((
                id,
                hash,
                tenant_id_str,
                name,
                key_type_str,
                rate_limit,
                created_at_str,
                last_used_str,
                expires_at_str,
                revoked,
                metadata_str,
            ))
        })?;

        let mut keys = Vec::new();
        for row in rows {
            let (
                id,
                hash,
                tenant_id_str,
                name,
                key_type_str,
                rate_limit,
                created_at_str,
                last_used_str,
                expires_at_str,
                revoked,
                metadata_str,
            ) = row?;
            let tenant_id: TenantId = tenant_id_str.parse()?;
            let key_type = string_to_api_key_type(&key_type_str);
            let created_at: DateTime<Utc> = created_at_str.parse()?;
            let last_used: Option<DateTime<Utc>> = last_used_str.and_then(|s| s.parse().ok());
            let expires_at: Option<DateTime<Utc>> = expires_at_str.and_then(|s| s.parse().ok());
            let metadata: HashMap<String, String> =
                serde_json::from_str(&metadata_str).unwrap_or_default();

            keys.push(ApiKey {
                id,
                hash,
                tenant_id,
                name,
                key_type,
                rate_limit: rate_limit as u32,
                created_at,
                last_used,
                expires_at,
                revoked: revoked != 0,
                metadata,
            });
        }
        Ok(keys)
    }

    /// Delete an API key by ID.
    pub fn delete_api_key(&self, key_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("poisoned lock in delete_api_key");
        conn.execute(
            "DELETE FROM enterprise_api_keys WHERE id = ?1",
            params![key_id],
        )?;
        Ok(())
    }

    // ─── Audit Log CRUD ───────────────────────────────────────────────────

    /// Insert an audit entry into the database.
    pub fn save_audit_entry(&self, entry: &AuditEntry) -> Result<()> {
        let conn = self.conn.lock().expect("poisoned lock in save_audit_entry");
        conn.execute(
            "INSERT INTO enterprise_audit_log
                (id, timestamp, tenant_id, user_id, action, resource,
                 resource_id, success, details, ip_address, user_agent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.id,
                entry.timestamp.to_rfc3339(),
                entry.tenant_id.to_string(),
                entry.user_id.map(|u| u.to_string()),
                entry.action.as_str(),
                entry.resource,
                entry.resource_id,
                entry.success as i64,
                entry.details,
                entry.ip_address,
                entry.user_agent,
            ],
        )?;
        Ok(())
    }

    /// Load audit entries, optionally filtered by tenant_id, ordered by timestamp DESC.
    ///
    /// Pass `limit = 0` to load all entries (use with caution on large datasets).
    pub fn load_audit_entries(
        &self,
        tenant_id: Option<&TenantId>,
        limit: usize,
    ) -> Result<Vec<AuditEntry>> {
        let (sql, has_tenant_filter) = if tenant_id.is_some() {
            (
                "SELECT id, timestamp, tenant_id, user_id, action, resource, resource_id,
                        success, details, ip_address, user_agent
                 FROM enterprise_audit_log
                 WHERE tenant_id = ?1
                 ORDER BY timestamp DESC",
                true,
            )
        } else {
            (
                "SELECT id, timestamp, tenant_id, user_id, action, resource, resource_id,
                        success, details, ip_address, user_agent
                 FROM enterprise_audit_log
                 ORDER BY timestamp DESC",
                false,
            )
        };

        let sql = if limit > 0 {
            format!("{} LIMIT {}", sql, limit)
        } else {
            sql.to_string()
        };

        let conn = self
            .conn
            .lock()
            .expect("poisoned lock in load_audit_entries");
        let mut stmt = conn.prepare(&sql)?;

        if has_tenant_filter {
            let rows = stmt.query_map(
                params![tenant_id
                    .expect(
                        "load_audit_entries: tenant_id must be set when has_tenant_filter is true"
                    )
                    .to_string()],
                row_to_audit_entry,
            )?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        } else {
            let rows = stmt.query_map([], row_to_audit_entry)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        }
    }

    /// Prune old audit entries, keeping only the `keep` most recent entries.
    /// Returns the number of deleted rows.
    pub fn prune_audit_log(&self, keep: usize) -> Result<usize> {
        let conn = self.conn.lock().expect("poisoned lock in prune_audit_log");
        let deleted = conn.execute(
            "DELETE FROM enterprise_audit_log WHERE id NOT IN (
                SELECT id FROM enterprise_audit_log ORDER BY timestamp DESC LIMIT ?1
            )",
            params![keep as i64],
        )?;
        Ok(deleted)
    }

    // ─── Rate Limit Config CRUD ───────────────────────────────────────────

    /// Save or update a rate limit configuration.
    pub fn save_rate_limit_config(
        &self,
        key: &RateLimitKey,
        config: &RateLimitConfig,
    ) -> Result<()> {
        let (key_type, key_value) = rate_limit_key_to_parts(key);
        let conn = self
            .conn
            .lock()
            .expect("poisoned lock in save_rate_limit_config");
        conn.execute(
            "INSERT INTO enterprise_rate_limits (key_type, key_value, rpm, burst)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key_type, key_value) DO UPDATE SET
                rpm = excluded.rpm,
                burst = excluded.burst",
            params![key_type, key_value, config.rpm as i64, config.burst as i64],
        )?;
        Ok(())
    }

    /// Load all rate limit configurations from the database.
    pub fn load_all_rate_limit_configs(&self) -> Result<Vec<(RateLimitKey, RateLimitConfig)>> {
        let conn = self
            .conn
            .lock()
            .expect("poisoned lock in load_all_rate_limit_configs");
        let mut stmt =
            conn.prepare("SELECT key_type, key_value, rpm, burst FROM enterprise_rate_limits")?;
        let rows = stmt.query_map([], |row| {
            let key_type: String = row.get(0)?;
            let key_value: String = row.get(1)?;
            let rpm: i64 = row.get(2)?;
            let burst: i64 = row.get(3)?;
            Ok((key_type, key_value, rpm, burst))
        })?;

        let mut configs = Vec::new();
        for row in rows {
            let (key_type, key_value, rpm, burst) = row?;
            let key = rate_limit_key_from_parts(&key_type, &key_value);
            let config = RateLimitConfig {
                rpm: rpm as u32,
                burst: burst as u32,
            };
            configs.push((key, config));
        }
        Ok(configs)
    }

    /// Delete a rate limit configuration.
    pub fn delete_rate_limit_config(&self, key: &RateLimitKey) -> Result<()> {
        let (key_type, key_value) = rate_limit_key_to_parts(key);
        let conn = self
            .conn
            .lock()
            .expect("poisoned lock in delete_rate_limit_config");
        conn.execute(
            "DELETE FROM enterprise_rate_limits WHERE key_type = ?1 AND key_value = ?2",
            params![key_type, key_value],
        )?;
        Ok(())
    }
}

// ─── Row mapping helper ──────────────────────────────────────────────────

fn row_to_audit_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditEntry> {
    let id: String = row.get(0)?;
    let timestamp_str: String = row.get(1)?;
    let tenant_id_str: String = row.get(2)?;
    let user_id_str: Option<String> = row.get(3)?;
    let action_str: String = row.get(4)?;
    let resource: String = row.get(5)?;
    let resource_id: Option<String> = row.get(6)?;
    let success: i64 = row.get(7)?;
    let details: Option<String> = row.get(8)?;
    let ip_address: Option<String> = row.get(9)?;
    let user_agent: Option<String> = row.get(10)?;

    let timestamp: DateTime<Utc> = timestamp_str.parse().unwrap_or_else(|_| Utc::now());
    let tenant_id: TenantId = tenant_id_str.parse().unwrap_or_else(|_| Uuid::nil());
    let user_id: Option<uuid::Uuid> = user_id_str.and_then(|s| s.parse().ok());
    let action = parse_audit_action(&action_str);

    Ok(AuditEntry {
        id,
        timestamp,
        tenant_id,
        user_id,
        action,
        resource,
        resource_id,
        success: success != 0,
        details,
        ip_address,
        user_agent,
    })
}

// ─── Enum conversion helpers ─────────────────────────────────────────────

fn plan_to_string(plan: &Plan) -> &str {
    match plan {
        Plan::Free => "Free",
        Plan::Pro => "Pro",
        Plan::Enterprise => "Enterprise",
    }
}

fn string_to_plan(s: &str) -> Plan {
    match s {
        "Free" => Plan::Free,
        "Pro" => Plan::Pro,
        "Enterprise" => Plan::Enterprise,
        _ => Plan::Free,
    }
}

fn api_key_type_to_string(key_type: &ApiKeyType) -> &str {
    match key_type {
        ApiKeyType::Live => "Live",
        ApiKeyType::Test => "Test",
    }
}

fn string_to_api_key_type(s: &str) -> ApiKeyType {
    match s {
        "Live" => ApiKeyType::Live,
        "Test" => ApiKeyType::Test,
        _ => ApiKeyType::Live,
    }
}

fn rate_limit_key_to_parts(key: &RateLimitKey) -> (&str, String) {
    match key {
        RateLimitKey::Tenant(id) => ("tenant", id.to_string()),
        RateLimitKey::ApiKey(id) => ("api_key", id.clone()),
        RateLimitKey::Ip(ip) => ("ip", ip.clone()),
    }
}

fn rate_limit_key_from_parts(key_type: &str, key_value: &str) -> RateLimitKey {
    match key_type {
        "tenant" => {
            let id: TenantId = key_value
                .parse()
                .expect("invalid tenant id in rate limit config");
            RateLimitKey::Tenant(id)
        }
        "api_key" => RateLimitKey::ApiKey(key_value.to_string()),
        "ip" => RateLimitKey::Ip(key_value.to_string()),
        _ => panic!("unknown rate limit key type: {}", key_type),
    }
}

fn parse_audit_action(s: &str) -> AuditAction {
    match s {
        "memory.search" => AuditAction::MemorySearch,
        "memory.add" => AuditAction::MemoryAdd,
        "memory.update" => AuditAction::MemoryUpdate,
        "memory.delete" => AuditAction::MemoryDelete,
        "memory.get" => AuditAction::MemoryGet,
        "tenant.create" => AuditAction::TenantCreate,
        "tenant.update" => AuditAction::TenantUpdate,
        "tenant.delete" => AuditAction::TenantDelete,
        "api_key.create" => AuditAction::ApiKeyCreate,
        "api_key.revoke" => AuditAction::ApiKeyRevoke,
        "rate_limit.exceeded" => AuditAction::RateLimitExceeded,
        "permission.denied" => AuditAction::PermissionDenied,
        "auth.login" => AuditAction::Login,
        "auth.logout" => AuditAction::Logout,
        other => AuditAction::Other(other.to_string()),
    }
}

/// Reconstruct the in-memory stores from the database.
///
/// Loads tenants, API keys, and rate limit configs from SQLite and populates
/// the corresponding stores. This is called at startup so that persisted
/// state survives restarts.
pub fn populate_stores_from_db(
    db: &EnterpriseDb,
    tenant_store: &mut crate::enterprise::tenant::TenantStore,
    api_key_store: &mut crate::enterprise::keys::ApiKeyStore,
    rate_limiter: &mut crate::enterprise::rate_limit::RateLimiter,
) -> Result<()> {
    // Load tenants
    let tenants = db.load_all_tenants()?;
    for (_, tenant) in tenants {
        tenant_store.insert_existing(tenant);
    }

    // Load API keys (builds tenant_keys index internally)
    let keys = db.load_all_api_keys()?;
    for key in keys {
        api_key_store.insert_existing(key);
    }

    // Load rate limit configs
    let configs = db.load_all_rate_limit_configs()?;
    for (key, config) in configs {
        rate_limiter.set_config(key, config);
    }

    Ok(())
}
