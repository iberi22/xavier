//! Mini-Expert Registry for local personal mini-experts.
//!
//! Two layers live here:
//! - `MiniExpertRegistry`: legacy JSON storage (segment, language, clearance,
//!   source dataset, GGUF path, endpoint). Kept for the settings-driven routes.
//! - `ExpertStore` (registry v2): SQLite, versioned experts with exactly one
//!   `active` version per name, domain centroids, and an invocation log.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::settings::types::MiniExpertConfig;

/// Persistent record representing a personal mini-expert.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MiniExpertEntry {
    pub name: String,
    pub segment: String,
    pub language: String,
    pub clearance: u8,
    pub source_dataset: String,
    pub model_gguf_path: String,
    pub provider: String,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl MiniExpertEntry {
    /// Returns the unique identifier for the mini-expert (maps to name).
    pub fn id(&self) -> &str {
        &self.name
    }

    /// Returns the source dataset ID.
    pub fn dataset_id(&self) -> &str {
        &self.source_dataset
    }

    /// Returns the GGUF model path.
    pub fn gguf_path(&self) -> &str {
        &self.model_gguf_path
    }

    /// Returns the Ollama model name (maps to name).
    pub fn ollama_model(&self) -> &str {
        &self.name
    }

    /// Converts a MiniExpertEntry to MiniExpertConfig for use in ProviderRouter.
    pub fn to_config(&self) -> MiniExpertConfig {
        MiniExpertConfig {
            name: self.name.clone(),
            provider: self.provider.clone(),
            endpoint: self.endpoint.clone(),
            api_key: self.api_key.clone(),
        }
    }
}

/// Persistent registry for mini-experts.
#[derive(Debug, Clone)]
pub struct MiniExpertRegistry {
    storage_path: PathBuf,
    entries: Arc<RwLock<Vec<MiniExpertEntry>>>,
}

impl MiniExpertRegistry {
    /// Returns the default storage path (`.xavier/mini_experts.json`).
    pub fn default_path() -> PathBuf {
        PathBuf::from(".xavier/mini_experts.json")
    }

    /// Returns the data storage path (`data/mini_experts.json`).
    pub fn data_path() -> PathBuf {
        PathBuf::from("data/mini_experts.json")
    }

    /// Creates a new MiniExpertRegistry backed by the given path and loads any existing records.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let storage_path = path.as_ref().to_path_buf();
        let registry = Self {
            storage_path,
            entries: Arc::new(RwLock::new(Vec::new())),
        };
        let _ = registry.reload();
        registry
    }

    /// Loads or creates the default registry at `.xavier/mini_experts.json`.
    pub fn load_default() -> Self {
        Self::new(Self::default_path())
    }

    /// Loads or creates the registry at `data/mini_experts.json`.
    pub fn load_data_path() -> Self {
        Self::new(Self::data_path())
    }

    /// Reloads entries from disk.
    pub fn reload(&self) -> Result<()> {
        if self.storage_path.exists() {
            let content = fs::read_to_string(&self.storage_path)
                .with_context(|| format!("Failed to read {}", self.storage_path.display()))?;
            if !content.trim().is_empty() {
                let loaded: Vec<MiniExpertEntry> = serde_json::from_str(&content)
                    .with_context(|| format!("Failed to parse {}", self.storage_path.display()))?;
                let mut guard = self
                    .entries
                    .write()
                    .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
                *guard = loaded;
            }
        }
        Ok(())
    }

    /// Saves current entries to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        let guard = self
            .entries
            .read()
            .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
        let json = serde_json::to_string_pretty(&*guard)?;
        fs::write(&self.storage_path, json)
            .with_context(|| format!("Failed to write {}", self.storage_path.display()))?;
        Ok(())
    }

    /// Registers or updates a mini-expert entry and persists to disk.
    pub fn register(&self, entry: MiniExpertEntry) -> Result<()> {
        {
            let mut guard = self
                .entries
                .write()
                .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
            if let Some(pos) = guard.iter().position(|e| e.name == entry.name) {
                guard[pos] = entry;
            } else {
                guard.push(entry);
            }
        }
        self.save()
    }

    /// Retrieves a mini-expert entry by name.
    pub fn get(&self, name: &str) -> Option<MiniExpertEntry> {
        let guard = self.entries.read().ok()?;
        guard.iter().find(|e| e.name == name).cloned()
    }

    /// Returns a list of all registered mini-experts.
    pub fn list(&self) -> Vec<MiniExpertEntry> {
        self.entries
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Removes a mini-expert entry by name and persists to disk.
    pub fn delete(&self, name: &str) -> Result<bool> {
        let removed = {
            let mut guard = self
                .entries
                .write()
                .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
            if let Some(pos) = guard.iter().position(|e| e.name == name) {
                guard.remove(pos);
                true
            } else {
                false
            }
        };

        if removed {
            self.save()?;
        }
        Ok(removed)
    }
}

// ---------------------------------------------------------------------------
// Registry v2 (SQLite)
// ---------------------------------------------------------------------------

/// Lifecycle status of one expert version.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExpertStatus {
    Candidate,
    Active,
    Retired,
}

impl ExpertStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Active => "active",
            Self::Retired => "retired",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "retired" => Self::Retired,
            _ => Self::Candidate,
        }
    }
}

/// One versioned row of the `mini_experts` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpertRecord {
    pub name: String,
    pub version: String,
    pub domain: String,
    pub base_model: String,
    pub bundle_hash: String,
    pub gguf_path: String,
    pub ollama_model: String,
    pub metrics_json: String,
    pub status: ExpertStatus,
    pub clearance: u8,
    pub created_at: String,
    pub language: String,
    pub source_dataset: String,
    /// `local`/`ollama` (served by Ollama) or another provider name (legacy HTTP invoke).
    pub provider: String,
    /// Provider endpoint; empty means the daemon's local Ollama.
    pub endpoint: String,
}

impl ExpertRecord {
    /// Legacy provider config (name, provider, endpoint) for `ProviderRouter`.
    pub fn to_config(&self) -> MiniExpertConfig {
        MiniExpertConfig {
            name: self.name.clone(),
            provider: self.provider.clone(),
            endpoint: if self.endpoint.trim().is_empty() {
                "http://localhost:11434/v1".to_string()
            } else {
                self.endpoint.clone()
            },
            api_key: None,
        }
    }
}

/// Input for registering a new expert version.
#[derive(Debug, Clone, Default)]
pub struct NewExpert {
    pub name: String,
    /// Auto-assigned (`vN`) when `None`.
    pub version: Option<String>,
    pub domain: String,
    pub base_model: String,
    pub bundle_hash: String,
    pub gguf_path: String,
    /// Defaults to `name:version`.
    pub ollama_model: Option<String>,
    /// Empty means `{}`.
    pub metrics_json: String,
    pub clearance: u8,
    pub language: String,
    pub source_dataset: String,
    /// Empty means `local`.
    pub provider: String,
    pub endpoint: String,
}

/// One logged invocation (feedback-loop input).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvocationRecord {
    pub id: i64,
    pub ts: String,
    pub expert: String,
    pub version: String,
    pub query_sha256: String,
    pub latency_ms: u64,
    pub ok: bool,
}

/// SQLite-backed registry v2. Cheap to clone; one connection behind a mutex.
#[derive(Clone)]
pub struct ExpertStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

const EXPERT_COLUMNS: &str = "name, version, domain, base_model, bundle_hash, gguf_path, \
     ollama_model, metrics_json, status, clearance, created_at, language, source_dataset, \
     provider, endpoint";

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExpertRecord> {
    let status: String = row.get(8)?;
    let clearance: i64 = row.get(9)?;
    Ok(ExpertRecord {
        name: row.get(0)?,
        version: row.get(1)?,
        domain: row.get(2)?,
        base_model: row.get(3)?,
        bundle_hash: row.get(4)?,
        gguf_path: row.get(5)?,
        ollama_model: row.get(6)?,
        metrics_json: row.get(7)?,
        status: ExpertStatus::parse(&status),
        clearance: clearance.clamp(0, 255) as u8,
        created_at: row.get(10)?,
        language: row.get(11)?,
        source_dataset: row.get(12)?,
        provider: row.get(13)?,
        endpoint: row.get(14)?,
    })
}

fn ensure_column(conn: &rusqlite::Connection, table: &str, column: &str, ddl: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|c| c == column);
    drop(stmt);
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"))?;
    }
    Ok(())
}

fn next_version_in(conn: &rusqlite::Connection, name: &str) -> Result<String> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mini_experts WHERE name = ?1",
        [name],
        |r| r.get(0),
    )?;
    let mut n = count + 1;
    loop {
        let v = format!("v{n}");
        let taken: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mini_experts WHERE name = ?1 AND version = ?2",
            [name, v.as_str()],
            |r| r.get(0),
        )?;
        if taken == 0 {
            return Ok(v);
        }
        n += 1;
    }
}

fn activate_in(conn: &rusqlite::Connection, name: &str, version: &str) -> Result<()> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mini_experts WHERE name = ?1 AND version = ?2",
        [name, version],
        |r| r.get(0),
    )?;
    if exists == 0 {
        anyhow::bail!("mini-expert {name}@{version} not found");
    }
    conn.execute(
        "UPDATE mini_experts SET status = 'retired' \
         WHERE name = ?1 AND status = 'active' AND version <> ?2",
        [name, version],
    )?;
    conn.execute(
        "UPDATE mini_experts SET status = 'active' WHERE name = ?1 AND version = ?2",
        [name, version],
    )?;
    Ok(())
}

impl ExpertStore {
    /// File name of the registry inside the daemon data dir.
    pub const DB_FILE: &'static str = "mini_experts.db";

    /// Opens (creating if needed) the store at `path`.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }
        }
        let conn = rusqlite::Connection::open(path)
            .with_context(|| format!("Failed to open {}", path.display()))?;
        Self::init(conn)
    }

    /// Opens the store in the daemon data dir and imports legacy JSON registries.
    ///
    /// The store is opened (and the legacy import run) once per data dir and then
    /// shared, so per-request callers do not reopen the DB or re-run DDL/imports.
    pub fn open_default() -> Result<Self> {
        static CACHE: Mutex<Option<(PathBuf, ExpertStore)>> = Mutex::new(None);
        let data_dir = crate::settings::XavierSettings::resolve_data_dir();
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((dir, store)) = cache.as_ref() {
            if *dir == data_dir {
                return Ok(store.clone());
            }
        }
        let store = Self::open_default_uncached(&data_dir)?;
        *cache = Some((data_dir, store.clone()));
        Ok(store)
    }

    fn open_default_uncached(data_dir: &Path) -> Result<Self> {
        let store = Self::open(data_dir.join(Self::DB_FILE))?;
        for legacy in [
            data_dir.join("mini_experts.json"),
            MiniExpertRegistry::data_path(),
            MiniExpertRegistry::default_path(),
        ] {
            if let Err(e) = store.import_json(&legacy) {
                tracing::warn!(
                    "mini-expert JSON import from {} failed: {e}",
                    legacy.display()
                );
            }
        }
        Ok(store)
    }

    fn init(conn: rusqlite::Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS mini_experts (
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                domain TEXT NOT NULL DEFAULT '',
                base_model TEXT NOT NULL DEFAULT '',
                bundle_hash TEXT NOT NULL DEFAULT '',
                gguf_path TEXT NOT NULL DEFAULT '',
                ollama_model TEXT NOT NULL DEFAULT '',
                metrics_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'candidate'
                    CHECK (status IN ('candidate','active','retired')),
                clearance INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                language TEXT NOT NULL DEFAULT '',
                source_dataset TEXT NOT NULL DEFAULT '',
                provider TEXT NOT NULL DEFAULT 'local',
                endpoint TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (name, version)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_experts_one_active
                ON mini_experts(name) WHERE status = 'active';
            CREATE TABLE IF NOT EXISTS mini_expert_centroids (
                domain TEXT PRIMARY KEY,
                text_sha256 TEXT NOT NULL,
                embedding TEXT NOT NULL,
                dimension INTEGER NOT NULL DEFAULT 0,
                model TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS mini_expert_invocations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts TEXT NOT NULL,
                expert TEXT NOT NULL,
                version TEXT NOT NULL,
                query_sha256 TEXT NOT NULL,
                latency_ms INTEGER NOT NULL,
                ok INTEGER NOT NULL
            );",
        )?;
        // Upgrade stores created before these columns existed.
        for (table, column, ddl) in [
            ("mini_experts", "language", "TEXT NOT NULL DEFAULT ''"),
            ("mini_experts", "source_dataset", "TEXT NOT NULL DEFAULT ''"),
            ("mini_experts", "provider", "TEXT NOT NULL DEFAULT 'local'"),
            ("mini_experts", "endpoint", "TEXT NOT NULL DEFAULT ''"),
            (
                "mini_expert_centroids",
                "dimension",
                "INTEGER NOT NULL DEFAULT 0",
            ),
            ("mini_expert_centroids", "model", "TEXT NOT NULL DEFAULT ''"),
        ] {
            ensure_column(&conn, table, column, ddl)?;
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Locks the connection; a poisoned mutex is recovered (the connection stays usable).
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
        Ok(self.conn.lock().unwrap_or_else(|e| {
            tracing::warn!("mini-expert store mutex was poisoned; recovering");
            e.into_inner()
        }))
    }

    /// Next auto version label (`vN`) for `name`.
    pub fn next_version(&self, name: &str) -> Result<String> {
        let conn = self.lock()?;
        next_version_in(&conn, name)
    }

    /// Registers a version as `candidate`; with `activate` it becomes the active one.
    /// Version allocation, insert and activation happen in one transaction.
    pub fn add_version(&self, new: NewExpert, activate: bool) -> Result<ExpertRecord> {
        if new.name.trim().is_empty() {
            anyhow::bail!("mini-expert name must not be empty");
        }
        for (field, value) in [
            ("name", Some(new.name.as_str())),
            ("version", new.version.as_deref()),
            ("domain", Some(new.domain.as_str())),
            ("gguf_path", Some(new.gguf_path.as_str())),
            ("ollama_model", new.ollama_model.as_deref()),
            ("provider", Some(new.provider.as_str())),
            ("endpoint", Some(new.endpoint.as_str())),
        ] {
            if value.is_some_and(|v| v.contains(['\r', '\n'])) {
                anyhow::bail!("mini-expert {field} must not contain line breaks");
            }
        }
        let metrics = if new.metrics_json.trim().is_empty() {
            "{}".to_string()
        } else {
            serde_json::from_str::<serde_json::Value>(&new.metrics_json)
                .context("metrics must be valid JSON")?;
            new.metrics_json.clone()
        };
        let provider = if new.provider.trim().is_empty() {
            "local".to_string()
        } else {
            new.provider.trim().to_string()
        };
        let version = {
            let mut conn = self.lock()?;
            let tx = conn.transaction()?;
            let version = match new.version.clone() {
                Some(v) if !v.trim().is_empty() => v,
                _ => next_version_in(&tx, &new.name)?,
            };
            let ollama_model = new
                .ollama_model
                .clone()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| format!("{}:{}", new.name, version));
            tx.execute(
                "INSERT INTO mini_experts (name, version, domain, base_model, bundle_hash, \
                 gguf_path, ollama_model, metrics_json, status, clearance, created_at, \
                 language, source_dataset, provider, endpoint) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'candidate', ?9, ?10, ?11, ?12, ?13, ?14)",
                rusqlite::params![
                    new.name,
                    version,
                    new.domain,
                    new.base_model,
                    new.bundle_hash,
                    new.gguf_path,
                    ollama_model,
                    metrics,
                    i64::from(new.clearance),
                    chrono::Utc::now().to_rfc3339(),
                    new.language,
                    new.source_dataset,
                    provider,
                    new.endpoint,
                ],
            )
            .with_context(|| format!("Failed to register {}@{}", new.name, version))?;
            if activate {
                activate_in(&tx, &new.name, &version)?;
            }
            tx.commit()?;
            version
        };
        self.get_version(&new.name, &version)?
            .ok_or_else(|| anyhow::anyhow!("version vanished after insert"))
    }

    /// Makes `version` the single active version of `name` (rollback = activate an older one).
    pub fn activate(&self, name: &str, version: &str) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        activate_in(&tx, name, version)?;
        tx.commit()?;
        Ok(())
    }

    /// Retires a version (the active one when `version` is `None`). Returns false if nothing matched.
    pub fn retire(&self, name: &str, version: Option<&str>) -> Result<bool> {
        let conn = self.lock()?;
        let n = match version {
            Some(v) => conn.execute(
                "UPDATE mini_experts SET status = 'retired' WHERE name = ?1 AND version = ?2",
                [name, v],
            )?,
            None => conn.execute(
                "UPDATE mini_experts SET status = 'retired' WHERE name = ?1 AND status = 'active'",
                [name],
            )?,
        };
        Ok(n > 0)
    }

    fn query(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<ExpertRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params, row_to_record)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All versions of all experts.
    pub fn list(&self) -> Result<Vec<ExpertRecord>> {
        self.query(
            &format!(
                "SELECT {EXPERT_COLUMNS} FROM mini_experts ORDER BY name, created_at, version"
            ),
            &[],
        )
    }

    /// Only the active version of each expert.
    pub fn list_active(&self) -> Result<Vec<ExpertRecord>> {
        self.query(
            &format!(
                "SELECT {EXPERT_COLUMNS} FROM mini_experts WHERE status = 'active' ORDER BY name"
            ),
            &[],
        )
    }

    /// All versions of one expert.
    pub fn versions(&self, name: &str) -> Result<Vec<ExpertRecord>> {
        self.query(
            &format!(
                "SELECT {EXPERT_COLUMNS} FROM mini_experts WHERE name = ?1 ORDER BY created_at, version"
            ),
            &[&name],
        )
    }

    pub fn get_version(&self, name: &str, version: &str) -> Result<Option<ExpertRecord>> {
        Ok(self
            .query(
                &format!(
                    "SELECT {EXPERT_COLUMNS} FROM mini_experts WHERE name = ?1 AND version = ?2"
                ),
                &[&name, &version],
            )?
            .into_iter()
            .next())
    }

    pub fn active(&self, name: &str) -> Result<Option<ExpertRecord>> {
        Ok(self
            .query(
                &format!(
                    "SELECT {EXPERT_COLUMNS} FROM mini_experts WHERE name = ?1 AND status = 'active'"
                ),
                &[&name],
            )?
            .into_iter()
            .next())
    }

    /// Imports a legacy JSON registry (read-only on the file). Names that already
    /// exist in the store are skipped, so the import is idempotent. Returns rows added.
    pub fn import_json(&self, path: &Path) -> Result<usize> {
        if !path.exists() {
            return Ok(0);
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if content.trim().is_empty() {
            return Ok(0);
        }
        let legacy: Vec<MiniExpertEntry> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        let mut added = 0;
        for entry in legacy {
            if !self.versions(&entry.name)?.is_empty() {
                continue;
            }
            self.add_version(
                NewExpert {
                    name: entry.name.clone(),
                    version: Some("v1".to_string()),
                    domain: entry.segment.clone(),
                    gguf_path: entry.model_gguf_path.clone(),
                    ollama_model: Some(entry.name.clone()),
                    language: entry.language.clone(),
                    source_dataset: entry.source_dataset.clone(),
                    provider: entry.provider.clone(),
                    endpoint: entry.endpoint.clone(),
                    metrics_json: serde_json::json!({
                        "imported_from": "json",
                        "language": entry.language,
                        "source_dataset": entry.source_dataset,
                    })
                    .to_string(),
                    clearance: entry.clearance,
                    ..Default::default()
                },
                true,
            )?;
            added += 1;
        }
        Ok(added)
    }

    /// Returns the stored centroid of `domain` only if it was computed from the same
    /// text by the same embedding `model` with `dimension` components; anything else
    /// is a cache miss (so a changed embedder recomputes instead of silently scoring 0).
    pub fn get_centroid(
        &self,
        domain: &str,
        text: &str,
        model: &str,
        dimension: usize,
    ) -> Result<Option<Vec<f32>>> {
        let conn = self.lock()?;
        let row: Option<(String, String, i64, String)> = conn
            .query_row(
                "SELECT text_sha256, embedding, dimension, model \
                 FROM mini_expert_centroids WHERE domain = ?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok();
        let Some((hash, emb, dim, stored_model)) = row else {
            return Ok(None);
        };
        if hash != sha256_hex(text) {
            return Ok(None);
        }
        if dim != dimension as i64 || stored_model != model {
            tracing::warn!(
                "mini-expert centroid for '{domain}' is stale (stored {stored_model}/{dim}d, \
                 current {model}/{dimension}d); recomputing"
            );
            return Ok(None);
        }
        let vec: Vec<f32> = serde_json::from_str(&emb).context("bad stored centroid")?;
        if vec.len() != dimension {
            return Ok(None);
        }
        Ok(Some(vec))
    }

    pub fn set_centroid(
        &self,
        domain: &str,
        text: &str,
        model: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO mini_expert_centroids (domain, text_sha256, embedding, dimension, model) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(domain) DO UPDATE SET text_sha256 = excluded.text_sha256, \
             embedding = excluded.embedding, dimension = excluded.dimension, model = excluded.model",
            rusqlite::params![
                domain,
                sha256_hex(text),
                serde_json::to_string(embedding)?,
                embedding.len() as i64,
                model
            ],
        )?;
        Ok(())
    }

    /// Logs one invocation (the query is stored only as its sha256).
    pub fn log_invocation(
        &self,
        expert: &str,
        version: &str,
        query: &str,
        latency_ms: u64,
        ok: bool,
    ) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO mini_expert_invocations (ts, expert, version, query_sha256, latency_ms, ok) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                chrono::Utc::now().to_rfc3339(),
                expert,
                version,
                sha256_hex(query),
                latency_ms as i64,
                i64::from(ok),
            ],
        )?;
        Ok(())
    }

    /// Most recent invocations first.
    pub fn invocations(&self, limit: usize) -> Result<Vec<InvocationRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, ts, expert, version, query_sha256, latency_ms, ok \
             FROM mini_expert_invocations ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(InvocationRecord {
                id: r.get(0)?,
                ts: r.get(1)?,
                expert: r.get(2)?,
                version: r.get(3)?,
                query_sha256: r.get(4)?,
                latency_ms: r.get::<_, i64>(5)?.max(0) as u64,
                ok: r.get::<_, i64>(6)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// Hex sha256 of a string.
pub fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    crate::utils::crypto::hex_encode(&Sha256::digest(text.as_bytes()))
}

// ---------------------------------------------------------------------------
// Ollama presence (serve)
// ---------------------------------------------------------------------------

/// Injectable Ollama operations so `serve` is testable without a real daemon.
#[async_trait::async_trait]
pub trait OllamaRunner: Send + Sync {
    /// Names of the models Ollama currently has.
    async fn list_models(&self) -> Result<Vec<String>>;
    /// `ollama create <model>` from a Modelfile body.
    async fn create(&self, model: &str, modelfile: &str) -> Result<()>;
}

/// Production runner: `/api/tags` over HTTP, `ollama create` via the CLI.
pub struct CliOllamaRunner {
    pub base_url: String,
}

impl CliOllamaRunner {
    pub fn from_env() -> Self {
        Self {
            base_url: crate::agents::expert_router::ollama_base_url_from_env(),
        }
    }
}

#[async_trait::async_trait]
impl OllamaRunner for CliOllamaRunner {
    async fn list_models(&self) -> Result<Vec<String>> {
        let body: serde_json::Value = reqwest::Client::new()
            .get(format!("{}/api/tags", self.base_url))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(body
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn create(&self, model: &str, modelfile: &str) -> Result<()> {
        let path = std::env::temp_dir().join(format!("xavier-modelfile-{}", ulid::Ulid::new()));
        fs::write(&path, modelfile)?;
        let out = tokio::process::Command::new("ollama")
            .arg("create")
            .arg(model)
            .arg("-f")
            .arg(&path)
            .output()
            .await;
        let _ = fs::remove_file(&path);
        let out = out.context("failed to run `ollama create`")?;
        if !out.status.success() {
            anyhow::bail!(
                "ollama create {model} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }
}

/// Outcome of ensuring one active expert exists in Ollama.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureOutcome {
    AlreadyPresent,
    Created,
    /// Missing in Ollama and not creatable (no readable GGUF) or creation failed.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureReport {
    pub name: String,
    pub version: String,
    pub ollama_model: String,
    pub outcome: EnsureOutcome,
}

/// Modelfile generated from a registry row (template comes from GGUF metadata).
pub fn render_modelfile(rec: &ExpertRecord) -> String {
    format!(
        "FROM {}\nPARAMETER temperature 0.2\nSYSTEM You are a personal mini-expert specialized in {}.\n",
        one_line(&rec.gguf_path),
        one_line(&rec.domain)
    )
}

/// Collapses line breaks so a value cannot inject extra Modelfile directives.
fn one_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn model_key(name: &str) -> &str {
    name.strip_suffix(":latest").unwrap_or(name)
}

/// Makes sure every active expert (or just `only`) is present in Ollama.
pub async fn ensure_active_in_ollama(
    store: &ExpertStore,
    runner: &dyn OllamaRunner,
    only: Option<&str>,
) -> Result<Vec<EnsureReport>> {
    let present = runner.list_models().await?;
    let mut reports = Vec::new();
    for rec in store.list_active()? {
        if only.is_some_and(|n| n != rec.name) {
            continue;
        }
        let outcome = if present
            .iter()
            .any(|m| model_key(m) == model_key(&rec.ollama_model))
        {
            EnsureOutcome::AlreadyPresent
        } else if rec.gguf_path.trim().is_empty() || !Path::new(&rec.gguf_path).exists() {
            EnsureOutcome::Failed(format!("GGUF not found at '{}'", rec.gguf_path))
        } else {
            match runner
                .create(&rec.ollama_model, &render_modelfile(&rec))
                .await
            {
                Ok(()) => EnsureOutcome::Created,
                Err(e) => EnsureOutcome::Failed(e.to_string()),
            }
        };
        reports.push(EnsureReport {
            name: rec.name,
            version: rec.version,
            ollama_model: rec.ollama_model,
            outcome,
        });
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("xavier_test_mexp_{}", ulid::Ulid::new()));
        let db_file = temp_dir.join("mini_experts.json");

        let registry = MiniExpertRegistry::new(&db_file);
        assert!(registry.list().is_empty());

        let entry = MiniExpertEntry {
            name: "test-expert".to_string(),
            segment: "codebase/f12".to_string(),
            language: "es".to_string(),
            clearance: 1,
            source_dataset: "f12-dataset-v1".to_string(),
            model_gguf_path: "/models/f12-expert.gguf".to_string(),
            provider: "local".to_string(),
            endpoint: "http://localhost:11434/v1".to_string(),
            api_key: None,
        };

        registry.register(entry.clone()).unwrap();

        // Verify get & list
        let fetched = registry.get("test-expert");
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.segment, "codebase/f12");
        assert_eq!(fetched.language, "es");
        assert_eq!(fetched.clearance, 1);
        assert_eq!(fetched.source_dataset, "f12-dataset-v1");
        assert_eq!(fetched.model_gguf_path, "/models/f12-expert.gguf");

        assert_eq!(registry.list().len(), 1);

        // Re-load from new instance to verify disk persistence
        let registry_reloaded = MiniExpertRegistry::new(&db_file);
        assert_eq!(registry_reloaded.list().len(), 1);
        assert_eq!(registry_reloaded.get("test-expert").unwrap(), entry);

        // Delete entry
        assert!(registry.delete("test-expert").unwrap());
        assert!(registry.get("test-expert").is_none());
        assert!(registry.list().is_empty());

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_mini_expert_entry_accessors() {
        let entry = MiniExpertEntry {
            name: "accessors-expert".to_string(),
            segment: "security".to_string(),
            language: "en".to_string(),
            clearance: 2,
            source_dataset: "sec-ds".to_string(),
            model_gguf_path: "/path/sec.gguf".to_string(),
            provider: "local".to_string(),
            endpoint: "http://localhost:11434".to_string(),
            api_key: None,
        };

        assert_eq!(entry.id(), "accessors-expert");
        assert_eq!(entry.dataset_id(), "sec-ds");
        assert_eq!(entry.gguf_path(), "/path/sec.gguf");
        assert_eq!(entry.ollama_model(), "accessors-expert");
    }

    #[test]
    fn test_data_path_registry() {
        assert_eq!(
            MiniExpertRegistry::data_path(),
            PathBuf::from("data/mini_experts.json")
        );
    }

    fn tmp_store() -> (tempfile::TempDir, ExpertStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        (dir, store)
    }

    fn new_expert(name: &str, version: Option<&str>) -> NewExpert {
        NewExpert {
            name: name.to_string(),
            version: version.map(String::from),
            domain: "rust code".to_string(),
            ..Default::default()
        }
    }

    fn active_count(store: &ExpertStore, name: &str) -> usize {
        store
            .versions(name)
            .unwrap()
            .iter()
            .filter(|r| r.status == ExpertStatus::Active)
            .count()
    }

    #[test]
    fn test_single_active_version_per_name() {
        let (_d, store) = tmp_store();
        store.add_version(new_expert("fx", None), true).unwrap();
        store.add_version(new_expert("fx", None), true).unwrap();
        store
            .add_version(new_expert("fx", Some("v9")), false)
            .unwrap();
        store.add_version(new_expert("other", None), true).unwrap();

        assert_eq!(active_count(&store, "fx"), 1);
        assert_eq!(store.active("fx").unwrap().unwrap().version, "v2");
        assert_eq!(store.active("other").unwrap().unwrap().version, "v1");

        // Rollback = activate the previous version; the displaced one is retired.
        store.activate("fx", "v1").unwrap();
        assert_eq!(active_count(&store, "fx"), 1);
        assert_eq!(store.active("fx").unwrap().unwrap().version, "v1");
        assert_eq!(
            store.get_version("fx", "v2").unwrap().unwrap().status,
            ExpertStatus::Retired
        );
        assert_eq!(
            store.get_version("fx", "v9").unwrap().unwrap().status,
            ExpertStatus::Candidate
        );
        assert!(store.activate("fx", "nope").is_err());
        assert_eq!(store.list_active().unwrap().len(), 2);
    }

    #[test]
    fn test_db_rejects_two_active_rows() {
        let (_d, store) = tmp_store();
        store.add_version(new_expert("fx", None), true).unwrap();
        store.add_version(new_expert("fx", None), false).unwrap();
        let conn = store.conn.lock().unwrap();
        let res = conn.execute(
            "UPDATE mini_experts SET status='active' WHERE name='fx' AND version='v2'",
            [],
        );
        assert!(
            res.is_err(),
            "unique partial index must reject a 2nd active row"
        );
    }

    #[test]
    fn test_retire_and_duplicate_version() {
        let (_d, store) = tmp_store();
        store
            .add_version(new_expert("fx", Some("v1")), true)
            .unwrap();
        assert!(store
            .add_version(new_expert("fx", Some("v1")), false)
            .is_err());
        assert!(store.retire("fx", None).unwrap());
        assert!(store.active("fx").unwrap().is_none());
        assert!(!store.retire("fx", None).unwrap());
        assert!(store
            .add_version(
                NewExpert {
                    metrics_json: "not json".into(),
                    ..new_expert("bad", None)
                },
                false
            )
            .is_err());
    }

    #[test]
    fn test_json_migration_is_idempotent_and_read_only() {
        let (dir, store) = tmp_store();
        let json_path = dir.path().join("legacy.json");
        let legacy = MiniExpertRegistry::new(&json_path);
        legacy
            .register(MiniExpertEntry {
                name: "old-exp".into(),
                segment: "codebase/f12".into(),
                language: "es".into(),
                clearance: 2,
                source_dataset: "ds".into(),
                model_gguf_path: "/m/old.gguf".into(),
                provider: "local".into(),
                endpoint: "http://localhost:11434/v1".into(),
                api_key: None,
            })
            .unwrap();
        let before = fs::read_to_string(&json_path).unwrap();

        assert_eq!(store.import_json(&json_path).unwrap(), 1);
        assert_eq!(store.import_json(&json_path).unwrap(), 0);
        assert_eq!(
            store.import_json(&dir.path().join("missing.json")).unwrap(),
            0
        );

        let rec = store.active("old-exp").unwrap().unwrap();
        assert_eq!(rec.version, "v1");
        assert_eq!(rec.domain, "codebase/f12");
        assert_eq!(rec.gguf_path, "/m/old.gguf");
        assert_eq!(rec.clearance, 2);
        assert_eq!(rec.ollama_model, "old-exp");
        assert_eq!(fs::read_to_string(&json_path).unwrap(), before);
    }

    #[test]
    fn test_centroid_cache_and_invocation_log() {
        let (_d, store) = tmp_store();
        assert!(store.get_centroid("d", "text", "m", 2).unwrap().is_none());
        store.set_centroid("d", "text", "m", &[1.0, 2.0]).unwrap();
        assert_eq!(
            store.get_centroid("d", "text", "m", 2).unwrap().unwrap(),
            vec![1.0, 2.0]
        );
        assert!(store
            .get_centroid("d", "other text", "m", 2)
            .unwrap()
            .is_none());

        store.log_invocation("fx", "v1", "hello", 12, true).unwrap();
        store.log_invocation("fx", "v1", "boom", 3, false).unwrap();
        let log = store.invocations(10).unwrap();
        assert_eq!(log.len(), 2);
        assert!(!log[0].ok);
        assert_eq!(log[1].query_sha256, sha256_hex("hello"));
        assert_ne!(log[1].query_sha256, "hello");
    }

    #[test]
    fn test_centroid_cache_misses_on_model_or_dimension_change() {
        let (_d, store) = tmp_store();
        store
            .set_centroid("d", "d", "model-a", &[1.0, 2.0])
            .unwrap();
        assert!(store
            .get_centroid("d", "d", "model-a", 2)
            .unwrap()
            .is_some());
        // Different embedder model, same dimension.
        assert!(store
            .get_centroid("d", "d", "model-b", 2)
            .unwrap()
            .is_none());
        // Same model id, different dimension.
        assert!(store
            .get_centroid("d", "d", "model-a", 3)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_add_persists_provider_endpoint_language_dataset() {
        let (_d, store) = tmp_store();
        let rec = store
            .add_version(
                NewExpert {
                    language: "es".into(),
                    source_dataset: "ds1".into(),
                    provider: "agy".into(),
                    endpoint: "http://gpu-box:11434/v1".into(),
                    ..new_expert("fx", None)
                },
                true,
            )
            .unwrap();
        assert_eq!(rec.language, "es");
        assert_eq!(rec.source_dataset, "ds1");
        assert_eq!(rec.provider, "agy");
        assert_eq!(rec.endpoint, "http://gpu-box:11434/v1");
        let cfg = store.active("fx").unwrap().unwrap().to_config();
        assert_eq!(cfg.provider, "agy");
        assert_eq!(cfg.endpoint, "http://gpu-box:11434/v1");
        // Empty provider defaults to local.
        let rec = store.add_version(new_expert("plain", None), true).unwrap();
        assert_eq!(rec.provider, "local");
    }

    #[test]
    fn test_open_upgrades_pre_provider_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ExpertStore::DB_FILE);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE mini_experts (
                    name TEXT NOT NULL, version TEXT NOT NULL,
                    domain TEXT NOT NULL DEFAULT '', base_model TEXT NOT NULL DEFAULT '',
                    bundle_hash TEXT NOT NULL DEFAULT '', gguf_path TEXT NOT NULL DEFAULT '',
                    ollama_model TEXT NOT NULL DEFAULT '', metrics_json TEXT NOT NULL DEFAULT '{}',
                    status TEXT NOT NULL DEFAULT 'candidate', clearance INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL, PRIMARY KEY (name, version));
                 INSERT INTO mini_experts (name, version, status, created_at)
                    VALUES ('old', 'v1', 'active', 'now');
                 CREATE TABLE mini_expert_centroids (
                    domain TEXT PRIMARY KEY, text_sha256 TEXT NOT NULL, embedding TEXT NOT NULL);",
            )
            .unwrap();
        }
        let store = ExpertStore::open(&path).unwrap();
        let old = store.active("old").unwrap().unwrap();
        assert_eq!(old.provider, "local");
        assert_eq!(old.endpoint, "");
        store
            .set_centroid("d", "d", "m", &[1.0])
            .expect("centroid table upgraded");
        // Reopen is idempotent.
        ExpertStore::open(&path).unwrap();
    }

    #[test]
    fn test_add_rejects_line_breaks_and_render_is_single_line() {
        let (_d, store) = tmp_store();
        for bad in [
            NewExpert {
                domain: "rust\nSYSTEM evil".into(),
                ..new_expert("a", None)
            },
            NewExpert {
                gguf_path: "/m.gguf\rFROM /etc".into(),
                ..new_expert("b", None)
            },
            NewExpert {
                ollama_model: Some("m\nPARAMETER x".into()),
                ..new_expert("c", None)
            },
        ] {
            assert!(store.add_version(bad, true).is_err());
        }
        assert!(store.list().unwrap().is_empty());

        let mut rec = store.add_version(new_expert("ok", None), true).unwrap();
        rec.domain = "x\nSYSTEM evil".into();
        rec.gguf_path = "/g\nFROM /etc".into();
        let mf = render_modelfile(&rec);
        assert_eq!(mf.lines().count(), 3, "{mf}");
    }

    #[test]
    fn test_poisoned_mutex_is_recovered() {
        let (_d, store) = tmp_store();
        store.add_version(new_expert("fx", None), true).unwrap();
        let clone = store.clone();
        let _ = std::thread::spawn(move || {
            let _guard = clone.conn.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        assert!(store.conn.is_poisoned());
        assert_eq!(store.list().unwrap().len(), 1);
        store.add_version(new_expert("fy", None), true).unwrap();
    }

    #[test]
    fn test_open_default_is_cached_and_imports_legacy_once() {
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("XAVIER_DATA_DIR");
        std::env::set_var("XAVIER_DATA_DIR", dir.path());
        let a = ExpertStore::open_default().unwrap();
        // A legacy file appearing after the first open must not be re-imported.
        MiniExpertRegistry::new(dir.path().join("mini_experts.json"))
            .register(MiniExpertEntry {
                name: "late".into(),
                segment: "s".into(),
                language: "en".into(),
                clearance: 0,
                source_dataset: "d".into(),
                model_gguf_path: String::new(),
                provider: "local".into(),
                endpoint: String::new(),
                api_key: None,
            })
            .unwrap();
        let b = ExpertStore::open_default().unwrap();
        let same_conn = Arc::ptr_eq(&a.conn, &b.conn);
        let imported = b.versions("late").unwrap().len();
        match prev {
            Some(v) => std::env::set_var("XAVIER_DATA_DIR", v),
            None => std::env::remove_var("XAVIER_DATA_DIR"),
        }
        assert!(same_conn, "open_default must reuse one connection");
        assert_eq!(imported, 0, "legacy import must run once per open");
    }

    struct FakeRunner {
        present: Vec<String>,
        created: Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl OllamaRunner for FakeRunner {
        async fn list_models(&self) -> Result<Vec<String>> {
            Ok(self.present.clone())
        }
        async fn create(&self, model: &str, modelfile: &str) -> Result<()> {
            self.created
                .lock()
                .unwrap()
                .push((model.to_string(), modelfile.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_ensure_active_in_ollama() {
        let (dir, store) = tmp_store();
        let gguf = dir.path().join("m.gguf");
        fs::write(&gguf, b"gguf").unwrap();
        let with_gguf = NewExpert {
            gguf_path: gguf.to_string_lossy().to_string(),
            ..new_expert("have-gguf", None)
        };
        store.add_version(with_gguf, true).unwrap();
        store
            .add_version(new_expert("no-gguf", None), true)
            .unwrap();
        store
            .add_version(
                NewExpert {
                    ollama_model: Some("present".into()),
                    ..new_expert("present-one", None)
                },
                true,
            )
            .unwrap();
        store.add_version(new_expert("cand", None), false).unwrap();

        let runner = FakeRunner {
            present: vec!["present:latest".into()],
            created: Mutex::new(vec![]),
        };
        let reports = ensure_active_in_ollama(&store, &runner, None)
            .await
            .unwrap();
        assert_eq!(reports.len(), 3);
        let by = |n: &str| {
            reports
                .iter()
                .find(|r| r.name == n)
                .unwrap()
                .outcome
                .clone()
        };
        assert_eq!(by("have-gguf"), EnsureOutcome::Created);
        assert_eq!(by("present-one"), EnsureOutcome::AlreadyPresent);
        assert!(matches!(by("no-gguf"), EnsureOutcome::Failed(_)));
        let created = runner.created.lock().unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].0, "have-gguf:v1");
        assert!(created[0].1.starts_with("FROM "));
    }
}
