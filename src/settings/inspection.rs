//! Side-effect-free settings inspection, using the runtime loader's merge rules.
use super::{serialization, XavierSettings};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ConfigError {
    pub code: i32,
    pub message: String,
}
type Result<T> = std::result::Result<T, ConfigError>;
fn failure(code: i32, message: impl Into<String>) -> ConfigError {
    ConfigError {
        code,
        message: message.into(),
    }
}

#[derive(Serialize)]
pub struct Problem {
    pub source: String,
    pub key: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct Entry {
    pub key: String,
    pub value: Value,
    pub source: String,
    pub env_var: Option<String>,
}

pub struct Inspection {
    pub config_path: PathBuf,
    pub runtime_state_path: PathBuf,
    defaults: Value,
    config: Value,
    runtime: Value,
    effective: Value,
    environment: BTreeMap<String, String>,
    pub problems: Vec<Problem>,
}

fn pointer(key: &str) -> String {
    format!(
        "/{}",
        key.split('.')
            .map(|part| part.replace('~', "~0").replace('/', "~1"))
            .collect::<Vec<_>>()
            .join("/")
    )
}
fn at<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.pointer(&pointer(key))
}
fn insert(value: &mut Value, key: &str, replacement: Value) {
    if let Some((head, tail)) = key.split_once('.') {
        if !value.is_object() {
            *value = json!({});
        }
        insert(&mut value[head], tail, replacement);
    } else {
        value[key] = replacement;
    }
}
fn leaves(value: &Value, prefix: &str, out: &mut BTreeMap<String, Value>) {
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            let key = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            leaves(value, &key, out);
        }
    } else {
        out.insert(prefix.to_owned(), value.clone());
    }
}

pub fn secret_key(key: &str) -> bool {
    key.split('.').any(|part| {
        let part = part.to_ascii_lowercase();
        part.split('_').any(|word| word == "key" || word == "keys")
            || part.contains("token")
            || part.contains("secret")
            || part.contains("password")
            || part.contains("credential")
            || part == "postgres_url"
            || part == "webhook_url"
    })
}
fn redact(value: &Value, key: &str) -> Value {
    if secret_key(key) {
        return json!(if value.is_null() || value.as_str() == Some("") {
            "unset"
        } else {
            "set"
        });
    }
    match value {
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), redact(v, k))).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(|v| redact(v, "")).collect()),
        _ => value.clone(),
    }
}

fn read_document(path: &Path, source: &str, problems: &mut Vec<Problem>) -> Value {
    match std::fs::read(path) {
        Ok(raw) => match serde_json::from_slice::<Value>(&raw) {
            Ok(value) if value.is_object() => value,
            _ => {
                problems.push(Problem {
                    source: source.into(),
                    key: String::new(),
                    message: "expected a JSON settings object".into(),
                });
                json!({})
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(_) => {
            problems.push(Problem {
                source: source.into(),
                key: String::new(),
                message: "cannot read settings file".into(),
            });
            json!({})
        }
    }
}

fn unknown_fields(schema: &Value, value: &Value, prefix: &str, fields: &mut Vec<String>) {
    match (schema, value) {
        (Value::Object(shape), Value::Object(object)) => {
            for (key, value) in object {
                let key_path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if let Some(child) = shape.get(key) {
                    unknown_fields(child, value, &key_path, fields);
                } else {
                    fields.push(key_path);
                }
            }
        }
        (Value::Array(shape), Value::Array(items)) => {
            if let Some(child) = shape.first() {
                for (index, value) in items.iter().enumerate() {
                    unknown_fields(child, value, &format!("{prefix}.{index}"), fields);
                }
            }
        }
        _ => {}
    }
}

fn accepts(defaults: &Value, key: &str, value: &Value) -> bool {
    let mut unknown = Vec::new();
    if let Some(schema) = at(defaults, key) {
        unknown_fields(schema, value, key, &mut unknown);
    }
    if !unknown.is_empty() {
        return false;
    }
    let mut probe = defaults.clone();
    insert(&mut probe, key, value.clone());
    serde_json::from_value::<XavierSettings>(probe).is_ok()
}
fn parse_value(defaults: &Value, key: &str, raw: &str) -> Result<Value> {
    if at(defaults, key).is_none() || at(defaults, key).is_some_and(Value::is_object) {
        return Err(failure(2, format!("unknown settings key: {key}")));
    }
    // Probe the actual deserializer, including Option<T>, integer bounds and enums.
    let string = Value::String(raw.to_owned());
    if accepts(defaults, key, &string) {
        return Ok(string);
    }
    let parsed: Value =
        serde_json::from_str(raw).map_err(|_| failure(2, format!("invalid type for {key}")))?;
    if !accepts(defaults, key, &parsed) {
        return Err(failure(2, format!("invalid type for {key}")));
    }
    Ok(parsed)
}

fn check_document(defaults: &Value, document: &Value, source: &str, problems: &mut Vec<Problem>) {
    fn visit(schema: &Value, value: &Value, prefix: &str, out: &mut Vec<(String, Value)>) {
        match (schema.as_object(), value.as_object()) {
            (Some(shape), Some(object)) => {
                for (key, value) in object {
                    let key_path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    if let Some(child) = shape.get(key) {
                        visit(child, value, &key_path, out);
                    } else {
                        out.push((key_path, value.clone()));
                    }
                }
            }
            _ => out.push((prefix.to_owned(), value.clone())),
        }
    }
    let mut unknown = Vec::new();
    unknown_fields(defaults, document, "", &mut unknown);
    for key in unknown {
        problems.push(Problem {
            source: source.into(),
            key,
            message: "unknown settings key".into(),
        });
    }
    let mut fields = Vec::new();
    visit(defaults, document, "", &mut fields);
    for (key, value) in fields {
        let message = if at(defaults, &key).is_none() {
            Some("unknown settings key")
        } else if !accepts(defaults, &key, &value) {
            Some("value does not match settings type")
        } else {
            None
        };
        if let Some(message) = message {
            if problems.iter().any(|p| p.key == key && p.source == source) {
                continue;
            }
            problems.push(Problem {
                source: source.into(),
                key,
                message: message.into(),
            });
        }
    }
}

fn relative_path(key: &str, value: &Value) -> bool {
    let field = key.rsplit('.').next().unwrap_or("");
    if !(field.ends_with("_path") || field.ends_with("_dir") || field.ends_with("_dirs")) {
        return false;
    }
    let values: Vec<&str> = if let Some(items) = value.as_array() {
        items.iter().filter_map(Value::as_str).collect()
    } else {
        value.as_str().into_iter().collect()
    };
    values
        .iter()
        .any(|s| !s.is_empty() && Path::new(s).is_relative())
}

impl Inspection {
    pub fn load() -> Self {
        let config_path = serialization::resolve_config_path();
        let runtime_state_path = serialization::resolve_runtime_state_path(&config_path);
        let mut problems = Vec::new();
        let defaults_settings = XavierSettings::default();
        let defaults = serde_json::to_value(&defaults_settings).expect("settings serialize");
        let config = read_document(&config_path, "config-file", &mut problems);
        let mut runtime = read_document(&runtime_state_path, "runtime-state", &mut problems);
        check_document(&defaults, &config, "config-file", &mut problems);
        check_document(&defaults, &runtime, "runtime-state", &mut problems);
        serialization::strip_secrets(&mut runtime);
        let base =
            serde_json::from_value::<XavierSettings>(config.clone()).unwrap_or(defaults_settings);
        let settings = serialization::merge_runtime_state(&runtime, base.clone()).unwrap_or(base);
        let mut effective = serde_json::to_value(settings).expect("settings serialize");
        let mut environment = BTreeMap::new();
        for &(key, names) in ENV_BINDINGS {
            for name in names.split('|') {
                if let Ok(raw) = std::env::var(name) {
                    match parse_value(&defaults, key, &raw) {
                        Ok(value) => {
                            insert(&mut effective, key, value);
                            environment.insert(key.into(), name.into());
                        }
                        Err(_) => problems.push(Problem {
                            source: "env".into(),
                            key: key.into(),
                            message: format!("invalid type in {name}"),
                        }),
                    }
                    break;
                }
            }
        }
        Self {
            config_path,
            runtime_state_path,
            defaults,
            config,
            runtime,
            effective,
            environment,
            problems,
        }
    }

    pub fn entry(&self, key: &str) -> Result<Entry> {
        let value = at(&self.effective, key)
            .filter(|v| !v.is_object())
            .ok_or_else(|| failure(2, format!("unknown settings key: {key}")))?;
        let source = if self.environment.contains_key(key) {
            "env"
        } else if at(&self.runtime, key).is_some() {
            "runtime-state"
        } else if at(&self.config, key).is_some() {
            "config-file"
        } else {
            "default"
        };
        Ok(Entry {
            key: key.into(),
            value: redact(value, key),
            source: source.into(),
            env_var: self.environment.get(key).cloned().or_else(|| {
                ENV_BINDINGS
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, n)| n.split('|').next().unwrap().into())
            }),
        })
    }
    pub fn entries(&self) -> Vec<Entry> {
        let mut fields = BTreeMap::new();
        leaves(&self.defaults, "", &mut fields);
        fields
            .keys()
            .filter_map(|key| self.entry(key).ok())
            .collect()
    }
    pub fn require_valid(&self) -> Result<()> {
        if !self.problems.is_empty() {
            return Err(failure(
                1,
                "invalid configuration; run xavier config validate",
            ));
        }
        Ok(())
    }
    pub fn validate(&mut self) {
        for (document, source) in [
            (&self.config, "config-file"),
            (&self.runtime, "runtime-state"),
            (&self.effective, "effective"),
        ] {
            let mut fields = BTreeMap::new();
            leaves(document, "", &mut fields);
            for (key, value) in fields {
                if relative_path(&key, &value)
                    && !self
                        .problems
                        .iter()
                        .any(|p| p.key == key && p.message.starts_with("relative path"))
                {
                    self.problems.push(Problem { source: source.into(), key, message: "relative path resolves against the current working directory; use an absolute path".into() });
                }
            }
        }
        if let Ok(settings) = serde_json::from_value::<XavierSettings>(self.effective.clone()) {
            if let Err(errors) = super::validation::validate_local_config(&settings) {
                for _ in errors {
                    self.problems.push(Problem { source: "effective".into(), key: "models".into(), message: "invalid local model configuration; check model URLs and embedding model".into() });
                }
            }
        }
    }
    pub fn set(&self, key: &str, raw: &str, secure_input: bool) -> Result<()> {
        self.entry(key)?;
        if secret_key(key) && !secure_input {
            return Err(failure(
                2,
                "sensitive settings require --value-file PATH or --value-stdin",
            ));
        }
        if serialization::is_secret_key(key.rsplit('.').next().unwrap_or(key)) {
            return Err(failure(2, "this credential is managed by the vault/environment and cannot be persisted in runtime state"));
        }
        let value = parse_value(&self.defaults, key, raw)?;
        if redact(&value, key) != value && !secure_input {
            return Err(failure(
                2,
                "sensitive settings require --value-file PATH or --value-stdin",
            ));
        }
        let mut stripped = value.clone();
        serialization::strip_secrets(&mut stripped);
        if stripped != value {
            return Err(failure(
                2,
                "nested credentials are managed by the vault/environment",
            ));
        }
        self.require_valid()?;
        let mut runtime = self.runtime.clone();
        insert(&mut runtime, key, value);
        self.persist(&runtime)
    }
    pub fn unset(&self, key: &str) -> Result<()> {
        self.entry(key)?;
        self.require_valid()?;
        let mut runtime = self.runtime.clone();
        let (parent, key) = key.rsplit_once('.').unwrap_or(("", key));
        let parent = if parent.is_empty() {
            Some(&mut runtime)
        } else {
            runtime.pointer_mut(&pointer(parent))
        };
        if let Some(object) = parent.and_then(Value::as_object_mut) {
            object.remove(key);
        }
        self.persist(&runtime)
    }
    fn persist(&self, runtime: &Value) -> Result<()> {
        if self.config_path == self.runtime_state_path
            || (self.config_path.canonicalize().ok().is_some()
                && self.config_path.canonicalize().ok()
                    == self.runtime_state_path.canonicalize().ok())
        {
            return Err(failure(
                2,
                "runtime state path must differ from the read-only config path",
            ));
        }
        let write = || -> anyhow::Result<()> {
            let parent = self
                .runtime_state_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            std::fs::create_dir_all(parent)?;
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                temp.as_file()
                    .set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            serde_json::to_writer_pretty(&mut temp, runtime)?;
            temp.write_all(b"\n")?;
            temp.as_file().sync_all()?;
            temp.persist(&self.runtime_state_path)?;
            #[cfg(unix)]
            std::fs::File::open(parent)?.sync_all()?;
            Ok(())
        };
        write().map_err(|_| failure(1, "failed to atomically write runtime state"))
    }
}

// Environment bindings exported by settings::env, plus the loader's credential aliases.
const ENV_BINDINGS: &[(&str, &str)] = &[
    ("server.host", "XAVIER_HOST"),
    ("server.port", "XAVIER_PORT"),
    ("server.log_level", "XAVIER_LOG_LEVEL"),
    ("server.code_graph_db_path", "XAVIER_CODE_GRAPH_DB_PATH"),
    ("server.url", "XAVIER_URL"),
    ("server.config_path", "XAVIER_CONFIG_PATH"),
    (
        "workspace.default_workspace_id",
        "XAVIER_DEFAULT_WORKSPACE_ID|XAVIER_WORKSPACE_ID",
    ),
    ("workspace.default_plan", "XAVIER_DEFAULT_PLAN"),
    (
        "workspace.storage_limit_bytes",
        "XAVIER_STORAGE_LIMIT_BYTES",
    ),
    ("workspace.request_limit", "XAVIER_REQUEST_LIMIT"),
    ("workspace.request_unit_limit", "XAVIER_REQUEST_UNIT_LIMIT"),
    (
        "workspace.embedding_provider_mode",
        "XAVIER_EMBEDDING_PROVIDER_MODE",
    ),
    (
        "workspace.managed_google_embeddings",
        "XAVIER_MANAGED_GOOGLE_EMBEDDINGS",
    ),
    ("retrieval.rrf_k", "XAVIER_RRF_K"),
    ("retrieval.zone_boost_multiplier", "XAVIER_ZONE_BOOST"),
    ("retrieval.zone_penalty_multiplier", "XAVIER_ZONE_PENALTY"),
    ("workspace.sync_policy", "XAVIER_SYNC_POLICY"),
    ("memory.backend", "XAVIER_MEMORY_BACKEND"),
    ("memory.data_dir", "XAVIER_DATA_DIR"),
    ("memory.embedding_dimensions", "XAVIER_EMBEDDING_DIMENSIONS"),
    ("memory.workspace_dir", "XAVIER_WORKSPACE_DIR"),
    ("memory.file_path", "XAVIER_MEMORY_FILE_PATH"),
    ("memory.sqlite_path", "XAVIER_MEMORY_SQLITE_PATH"),
    ("memory.vec_path", "XAVIER_MEMORY_VEC_PATH"),
    ("memory.supabase_url", "XAVIER_SUPABASE_URL"),
    ("memory.supabase_key", "XAVIER_SUPABASE_KEY"),
    ("memory.postgres_url", "XAVIER_POSTGRES_URL"),
    (
        "memory_layers.working.capacity",
        "XAVIER_WORKING_MEMORY_CAPACITY",
    ),
    (
        "memory_layers.working.lru_exempt_access_threshold",
        "XAVIER_WORKING_LRU_THRESHOLD",
    ),
    ("memory_layers.working.bm25_k1", "XAVIER_WORKING_BM25_K1"),
    ("memory_layers.working.bm25_b", "XAVIER_WORKING_BM25_B"),
    (
        "memory_layers.episodic.summary_window",
        "XAVIER_EPISODIC_SUMMARY_WINDOW",
    ),
    (
        "memory_layers.episodic.max_sessions",
        "XAVIER_MAX_EPISODIC_SESSIONS",
    ),
    (
        "memory_layers.episodic.min_event_importance",
        "XAVIER_EPISODIC_MIN_EVENT_IMPORTANCE",
    ),
    (
        "memory_layers.episodic.llm_summary_enabled",
        "XAVIER_EPISODIC_LLM_SUMMARY_ENABLED",
    ),
    ("models.api_flavor", "XAVIER_API_FLAVOR"),
    ("models.local_llm_model", "XAVIER_LOCAL_LLM_MODEL"),
    ("models.embedding_url", "XAVIER_EMBEDDING_URL"),
    ("models.embedding_model", "XAVIER_EMBEDDING_MODEL"),
    (
        "models.router_retrieved_model",
        "XAVIER_ROUTER_RETRIEVED_MODEL",
    ),
    ("models.router_complex_model", "XAVIER_ROUTER_COMPLEX_MODEL"),
    ("models.router_fast_model", "XAVIER_ROUTER_FAST_MODEL"),
    ("models.router_quality_model", "XAVIER_ROUTER_QUALITY_MODEL"),
    ("models.llm_model", "XAVIER_LLM_MODEL"),
    ("models.cloud_llm_model", "XAVIER_CLOUD_LLM_MODEL"),
    ("retrieval.disable_hyde", "XAVIER_DISABLE_HYDE"),
    ("sync.interval_ms", "XAVIER_SYNC_INTERVAL_MS"),
    ("sync.lag_threshold_ms", "XAVIER_SYNC_LAG_THRESHOLD_MS"),
    (
        "sync.save_ok_rate_threshold",
        "XAVIER_SYNC_SAVE_OK_RATE_THRESHOLD",
    ),
    ("sync.max_retries", "XAVIER_SYNC_MAX_RETRIES"),
    ("sync.retry_delay_ms", "XAVIER_SYNC_RETRY_DELAY_MS"),
    (
        "sync.min_health_interval_ms",
        "XAVIER_SYNC_MIN_HEALTH_INTERVAL_MS",
    ),
    ("sync.timeout_ms", "XAVIER_SYNC_TIMEOUT_MS"),
    ("embedding.embedder", "XAVIER_EMBEDDER"),
    ("embedding.gllm_model", "XAVIER_GLLM_MODEL"),
    ("embedding.api_flavor", "XAVIER_EMBEDDING_API_FLAVOR"),
    ("embedding.gllm_dimension", "XAVIER_GLLM_DIMENSION"),
    ("embedding.cache_enabled", "XAVIER_EMBEDDING_CACHE_ENABLED"),
    ("embedding.cache_size", "XAVIER_EMBEDDING_CACHE_SIZE"),
    (
        "embedding.cache_ttl_hours",
        "XAVIER_EMBEDDING_CACHE_TTL_HOURS",
    ),
    ("embedding.cache_db_path", "XAVIER_EMBEDDING_CACHE_DB_PATH"),
    ("embedding.timeout_secs", "XAVIER_EMBEDDING_TIMEOUT_SECS"),
    ("security.allowed_domains", "XAVIER_ALLOWED_DOMAINS"),
    (
        "security.encryption_at_rest_enabled",
        "XAVIER_ENCRYPTION_AT_REST_ENABLED",
    ),
    ("security.master_key_name", "XAVIER_MASTER_KEY_NAME"),
    ("telegram.enabled", "XAVIER_TELEGRAM_ENABLED"),
    ("router.policy_path", "XAVIER_ROUTER_POLICY_PATH"),
    (
        "router.policy_refresh_secs",
        "XAVIER_ROUTER_POLICY_REFRESH_SECS",
    ),
    ("chronicle.model", "Xavier_CHRONICLE_MODEL"),
    ("tgd.enabled", "XAVIER_TGD_ENABLED"),
    ("tgd.schedule", "XAVIER_TGD_SCHEDULE"),
    (
        "tgd.confidence_threshold",
        "XAVIER_TGD_CONFIDENCE_THRESHOLD",
    ),
    ("tgd.min_new_history", "XAVIER_TGD_MIN_NEW_HISTORY"),
    ("tgd.iterations", "XAVIER_TGD_ITERATIONS"),
    ("tgd.learning_rate", "XAVIER_TGD_LEARNING_RATE"),
    (
        "tgd.refinement_threshold",
        "XAVIER_TGD_REFINEMENT_THRESHOLD",
    ),
    (
        "tgd.refinement_batch_size",
        "XAVIER_TGD_REFINEMENT_BATCH_SIZE",
    ),
    ("enterprise.db_path", "XAVIER_ENTERPRISE_DB_PATH"),
    ("agents.weekly_budget", "XAVIER_WEEKLY_BUDGET"),
    ("advanced.qjl_threshold", "XAVIER_QJL_THRESHOLD"),
    (
        "advanced.entity_extraction_enabled",
        "XAVIER_ENTITY_EXTRACTION_ENABLED",
    ),
    ("advanced.audit_chain_enabled", "XAVIER_AUDIT_CHAIN_ENABLED"),
    ("advanced.panel_store_dir", "XAVIER_PANEL_STORE_DIR"),
    (
        "pgheart.instance_id",
        "PGHEART_INSTANCE_ID|XAVIER_PGHEART_INSTANCE_ID",
    ),
    ("pgheart.sync_interval_ms", "PGHEART_SYNC_INTERVAL_MS"),
    ("pgheart.auto_heartbeat", "PGHEART_AUTO_HEARTBEAT"),
    ("models.provider", "XAVIER_MODEL_PROVIDER|XAVIER_PROVIDER"),
    ("models.local_llm_url", "XAVIER_LOCAL_LLM_URL"),
    ("models.cloud_llm_url", "XAVIER_CLOUD_LLM_URL"),
    ("models.local_anthropic_url", "XAVIER_LOCAL_ANTHROPIC_URL"),
    ("models.llm_api_key", "XAVIER_LLM_API_KEY"),
    ("models.local_llm_api_key", "XAVIER_LOCAL_LLM_API_KEY"),
    ("security.token_secret", "XAVIER_TOKEN_SECRET"),
    ("telegram.bot_token", "XAVIER_TELEGRAM_TOKEN"),
    (
        "embedding.api_key",
        "XAVIER_EMBEDDING_API_KEY|XAVIER_OPENROUTER_API_KEY",
    ),
    (
        "telegram.notification_chat_id",
        "XAVIER_TELEGRAM_NOTIFICATION_CHAT_ID",
    ),
    ("pgheart.url", "PGHEART_URL|XAVIER_PGHEART_URL"),
    ("pgheart.token", "PGHEART_TOKEN|XAVIER_PGHEART_TOKEN"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_environment_bindings_are_known_settings_paths() {
        let defaults = serde_json::to_value(XavierSettings::default()).unwrap();
        let mut seen = std::collections::HashSet::new();
        for (key, _) in ENV_BINDINGS {
            assert!(at(&defaults, key).is_some(), "unknown binding {key}");
            assert!(seen.insert(key), "duplicate binding {key}");
        }
    }
}
