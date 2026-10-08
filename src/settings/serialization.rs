//! Logical concern: Serialization and path resolution for Xavier settings.
//!
//! This module handles loading configurations from files and resolving system paths.
//!
//! # Two files, two jobs (#2801)
//!
//! - **Defaults**: the config file (`config/xavier.config.json`) is *read-only
//!   defaults*. It lives in the git tree, so a write to it is a diff in a public
//!   repo — the daemon used to rewrite it in place (learned-policy float drift,
//!   a dirty `git status` on every boot, and a `git add -A` that commits live
//!   daemon state). Nothing in the daemon writes it any more.
//! - **Runtime state**: everything `save()` persists goes to the state dir
//!   (`$XDG_STATE_HOME/xavier/xavier.runtime.json`, override with
//!   `XAVIER_RUNTIME_STATE_PATH`), and is overlaid on top of the defaults at
//!   load time. On a clean machine the state file does not exist yet and the
//!   defaults stand on their own.
//!
//! The state file is keyed by the config path it belongs to, so an explicitly
//! pinned `XAVIER_CONFIG_PATH` (tests, per-instance deployments) never inherits
//! another instance's state.
//!
//! Environment variables still win over both: `current()` applies them last.

use super::types::XavierSettings;
use crate::secrets::vault::HardwareVault;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::fs;

const DEFAULT_CONFIG_PATH: &str = "config/xavier.config.json";
/// Tracked template the untracked runtime config is seeded from (#2801).
const CONFIG_TEMPLATE_PATH: &str = "config/xavier.config.example.json";
const RUNTIME_STATE_FILENAME: &str = "xavier.runtime.json";
const RUNTIME_STATE_DIRNAME: &str = ".xavier-state";

/// Resolve config path.
pub fn resolve_config_path() -> PathBuf {
    if let Ok(env_path) = std::env::var("XAVIER_CONFIG_PATH") {
        return PathBuf::from(env_path);
    }

    if let Some(config_dir) = dirs::config_dir() {
        // Priority 1: ~/.config/xavier/xavier.toml (or OS equivalent)
        let xavier_toml = config_dir.join("xavier").join("xavier.toml");
        if xavier_toml.exists() {
            return xavier_toml;
        }
        // Priority 2: ~/.config/xavier/xavier.config.json
        let xavier_json = config_dir.join("xavier").join("xavier.config.json");
        if xavier_json.exists() {
            return xavier_json;
        }
    }

    // Fallback to local project config
    PathBuf::from(DEFAULT_CONFIG_PATH)
}

/// Resolve data dir.
pub fn resolve_data_dir() -> PathBuf {
    if let Ok(env_path) = std::env::var("XAVIER_DATA_DIR") {
        return PathBuf::from(env_path);
    }

    if let Some(data_dir) = dirs::data_dir() {
        let resolved = data_dir.join("xavier");
        crate::test_support::guard_user_path(&resolved);
        return resolved;
    }

    PathBuf::from("data")
}

/// Load the read-only defaults file (the versioned config, if any).
fn parse_settings(path: &Path) -> Result<XavierSettings> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;

    // Handle both JSON and YAML/TOML if we want, but for now stick to what we have
    let parsed = if path.extension().is_some_and(|ext| ext == "toml") {
        // We need a TOML parser if we want to support .toml
        // But for now, let's assume JSON as per current implementation
        serde_json::from_str::<XavierSettings>(&raw).with_context(|| {
            format!(
                "failed to parse TOML config (as JSON) at {}",
                path.display()
            )
        })?
    } else {
        serde_json::from_str::<XavierSettings>(&raw)
            .with_context(|| format!("failed to parse config file at {}", path.display()))?
    };

    Ok(parsed)
}

/// Load defaults, then overlay the runtime state written by `save()`.
///
/// `None` only when the defaults file is absent AND no runtime state exists:
/// the clean-machine case, where the compiled-in defaults apply and the caller
/// decides what to do (main.rs skips config validation entirely). A pinned
/// config that is not on disk yet is NOT a reason to drop existing state: the
/// compiled-in defaults become the base and the state file is overlaid on top.
/// A malformed state file is reported but never takes the defaults down.
///
/// A *schema-incompatible* state file is the same class of problem, not a
/// different one: `merge_runtime_state` returns Err when the overlay deserializes
/// into something that is not a `XavierSettings` — the realistic case being a
/// binary that changed the type of a field while an older state file is still on
/// disk. That Err used to propagate straight to `main.rs`'s
/// `XavierSettings::load()?`, which aborted the whole process: a corrupt state
/// file took the server down with it. The daemon now boots on the defaults and
/// the failure is reported instead (D10).
pub fn load() -> Result<Option<XavierSettings>> {
    let path = resolve_config_path();
    seed_config_from_template(&path);
    let defaults = if path.exists() {
        Some(parse_settings(&path)?)
    } else {
        None
    };

    let state = match load_runtime_state(&path) {
        Ok(state) => state,
        Err(e) => {
            report_state_rejection("unreadable", &path, &e);
            None
        }
    };

    // A state document that exists but cannot be merged must not abort startup:
    // fall back to the defaults and keep every other setting intact.
    match (defaults, state) {
        (Some(defaults), Some(state)) => match merge_runtime_state(&state, defaults.clone()) {
            Ok(merged) => Ok(Some(merged)),
            Err(e) => {
                report_state_rejection("incompatible", &path, &e);
                Ok(Some(defaults))
            }
        },
        (Some(defaults), None) => Ok(Some(defaults)),
        (None, state) => match state {
            Some(state) => match merge_runtime_state(&state, XavierSettings::default()) {
                Ok(merged) => Ok(Some(merged)),
                Err(e) => {
                    report_state_rejection("incompatible", &path, &e);
                    Ok(None)
                }
            },
            None => Ok(None),
        },
    }
}

/// First run (#2801): `config/xavier.config.json` is untracked and gitignored,
/// so a fresh checkout does not have it. Seed it from the tracked template.
///
/// Only the in-tree default path is seeded (an explicitly pinned or per-user
/// config is the operator's own file), and only when it is absent: an existing
/// file is never touched. Best effort: a read-only checkout just runs on the
/// compiled-in defaults.
fn seed_config_from_template(path: &Path) {
    if path.exists() || path != Path::new(DEFAULT_CONFIG_PATH) {
        return;
    }
    let template = Path::new(CONFIG_TEMPLATE_PATH);
    if !template.exists() {
        return;
    }
    if let Err(e) = std::fs::copy(template, path) {
        tracing::debug!("could not seed {} from template: {e}", path.display());
    }
}

/// True for JSON keys that hold a credential (D11). Matched by name so a field
/// added later (or one nested in a list, like `mini_experts[].api_key`) is
/// covered without touching this list.
fn is_secret_key(key: &str) -> bool {
    key == "api_key"
        || key == "token"
        || key == "password"
        || key == "postgres_url"
        || key == "supabase_key"
        || key.ends_with("_api_key")
        || key.ends_with("_token")
        || key.ends_with("_secret")
}

/// Remove every credential from a settings document, recursively.
///
/// Secrets live in the vault / environment (`current()` reads them from there);
/// the runtime state keeps no value and no reference, so rotating them in `.env`
/// takes effect and a leaked state file leaks nothing.
fn strip_secrets(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|k, _| !is_secret_key(k));
            map.values_mut().for_each(strip_secrets);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_secrets),
        _ => {}
    }
}

/// Surface a rejected runtime state.
///
/// `load()` runs in `main.rs` *before* the tracing subscriber is installed, so a
/// `tracing::warn!` alone is dropped on the floor for exactly the failure we most
/// need to see. Write to stderr first, then mirror it into tracing for the cases
/// where a subscriber does exist (daemon, tests, library callers).
fn report_state_rejection(kind: &str, config_path: &Path, error: &anyhow::Error) {
    let state_path = resolve_runtime_state_path(config_path);
    let message = format!(
        "Xavier runtime state at {} was rejected ({kind}); continuing with defaults: {error:#}",
        state_path.display(),
    );
    eprintln!("WARN: {message}");
    tracing::warn!("{message}");
}

/// Read the runtime-state overlay that belongs to a given defaults file.
fn load_runtime_state(config_path: &Path) -> Result<Option<Value>> {
    let state_path = resolve_runtime_state_path(config_path);
    if !state_path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&state_path)
        .with_context(|| format!("failed to read runtime state at {}", state_path.display()))?;
    let mut parsed: Value = serde_json::from_str(&raw).with_context(|| {
        format!(
            "failed to parse runtime state file at {}",
            state_path.display()
        )
    })?;
    // States written before D11 carry secret values that would pin them over
    // the environment: ignore them.
    strip_secrets(&mut parsed);
    Ok(Some(parsed))
}

/// Overlay a runtime-state document onto the defaults.
///
/// Both sides deserialize into `XavierSettings` (every section is
/// `#[serde(default)]`), so this is a whole-struct replacement of the keys the
/// state file actually carries — a field absent from the state keeps its default
/// value instead of being zeroed.
fn merge_runtime_state(state: &Value, defaults: XavierSettings) -> Result<XavierSettings> {
    let base = serde_json::to_value(&defaults)
        .context("failed to serialize default settings for state overlay")?;
    let merged = deep_merge(base, state.clone());
    serde_json::from_value(merged)
        .context("runtime state does not fit the settings schema; keeping defaults")
}

/// Recursive JSON object merge: `overlay` wins for keys it defines.
fn deep_merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base_map), Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                let merged = match base_map.remove(&key) {
                    Some(base_value) => deep_merge(base_value, overlay_value),
                    None => overlay_value,
                };
                base_map.insert(key, merged);
            }
            Value::Object(base_map)
        }
        (_, overlay) => overlay,
    }
}

/// Current.
pub fn current() -> XavierSettings {
    let mut settings = load().ok().flatten().unwrap_or_default();
    settings.auth_token = std::env::var("XAVIER_TOKEN")
        .ok()
        .or_else(|| std::env::var("XAVIER_AUTH_TOKEN").ok());
    // Populate sensitive fields from env if not set via config file
    if settings.security.token_secret.is_none() {
        settings.security.token_secret = std::env::var("XAVIER_TOKEN_SECRET").ok();
    }
    if settings.telegram.bot_token.is_none() {
        settings.telegram.bot_token = std::env::var("XAVIER_TELEGRAM_TOKEN").ok();
    }
    if settings.telegram.bot_token.is_none() {
        let vault = HardwareVault::new("xavier-telegram");
        settings.telegram.bot_token = vault.get_secret("bot_token").ok();
    }
    if settings.telegram.notification_chat_id.is_none() {
        settings.telegram.notification_chat_id =
            std::env::var("XAVIER_TELEGRAM_NOTIFICATION_CHAT_ID").ok();
    }
    if settings.telegram.notification_chat_id.is_none() {
        let vault = HardwareVault::new("xavier-telegram");
        settings.telegram.notification_chat_id = vault.get_secret("notification_chat_id").ok();
    }
    // PGHEART_* is canonical; XAVIER_PGHEART_* (documented in .env.example) is an alias.
    let pgheart_env = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                std::env::var(format!("XAVIER_{name}"))
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            })
    };
    if settings.pgheart.url.is_none() {
        settings.pgheart.url = pgheart_env("PGHEART_URL");
    }
    if settings.pgheart.token.is_none() {
        settings.pgheart.token = pgheart_env("PGHEART_TOKEN");
    }
    if settings.pgheart.instance_id.is_none() {
        settings.pgheart.instance_id = pgheart_env("PGHEART_INSTANCE_ID");
    }
    if settings.models.llm_api_key.is_none() {
        settings.models.llm_api_key = std::env::var("XAVIER_LLM_API_KEY").ok();
    }
    if settings.models.local_llm_api_key.is_none() {
        settings.models.local_llm_api_key = std::env::var("XAVIER_LOCAL_LLM_API_KEY").ok();
    }
    if let Ok(data_dir) = std::env::var("XAVIER_DATA_DIR") {
        settings.memory.data_dir = data_dir;
    }
    if let Ok(workspace_dir) = std::env::var("XAVIER_WORKSPACE_DIR") {
        settings.memory.workspace_dir = workspace_dir;
    }
    if let Ok(file_path) = std::env::var("XAVIER_MEMORY_FILE_PATH") {
        settings.memory.file_path = file_path;
    }
    if let Ok(sqlite_path) = std::env::var("XAVIER_MEMORY_SQLITE_PATH") {
        settings.memory.sqlite_path = sqlite_path;
    }
    if let Ok(vec_path) = std::env::var("XAVIER_MEMORY_VEC_PATH") {
        settings.memory.vec_path = vec_path;
    }
    if let Ok(dimensions) = std::env::var("XAVIER_EMBEDDING_DIMENSIONS") {
        if let Ok(v) = dimensions.parse() {
            settings.memory.embedding_dimensions = v;
        }
    }
    if let Ok(embedding_url) = std::env::var("XAVIER_EMBEDDING_URL") {
        settings.models.embedding_url = embedding_url;
    }
    if settings.embedding.api_key.is_none() {
        settings.embedding.api_key = std::env::var("XAVIER_EMBEDDING_API_KEY")
            .ok()
            .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok());
    }
    // Retrieval fallbacks
    if settings.retrieval.rrf_k.is_none() {
        settings.retrieval.rrf_k = std::env::var("XAVIER_RRF_K")
            .ok()
            .and_then(|v| v.parse().ok());
    }
    // Sync fallbacks
    if let Ok(val) = std::env::var("XAVIER_SYNC_INTERVAL_MS") {
        if let Ok(v) = val.parse() {
            settings.sync.interval_ms = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_LAG_THRESHOLD_MS") {
        if let Ok(v) = val.parse() {
            settings.sync.lag_threshold_ms = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_SAVE_OK_RATE_THRESHOLD") {
        if let Ok(v) = val.parse() {
            settings.sync.save_ok_rate_threshold = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_MAX_RETRIES") {
        if let Ok(v) = val.parse() {
            settings.sync.max_retries = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_MIN_HEALTH_INTERVAL_MS") {
        if let Ok(v) = val.parse() {
            settings.sync.min_health_interval_ms = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_TIMEOUT_MS") {
        if let Ok(v) = val.parse() {
            settings.sync.timeout_ms = v;
        }
    }
    if settings.retrieval.zone_boost_multiplier.is_none() {
        settings.retrieval.zone_boost_multiplier = std::env::var("XAVIER_ZONE_BOOST")
            .ok()
            .and_then(|v| v.parse().ok());
    }
    if settings.retrieval.zone_penalty_multiplier.is_none() {
        settings.retrieval.zone_penalty_multiplier = std::env::var("XAVIER_ZONE_PENALTY")
            .ok()
            .and_then(|v| v.parse().ok());
    }
    settings
}

/// Resolve the path where runtime (execution) state is persisted.
///
/// Never the versioned config file: `config/xavier.config.json` is read-only
/// defaults, and a daemon that rewrites it turns every boot into a diff in a
/// public repo (#2801).
///
/// Priority:
/// 1. `XAVIER_RUNTIME_STATE_PATH` — explicit pin (per-instance deployments).
/// 2. When `XAVIER_CONFIG_PATH` is pinned, `<its dir>/.xavier-state/` — an
///    explicitly pinned config means "my own instance", so its state must not
///    be shared with the machine's node. This is what keeps a test (or a second
///    instance) from inheriting the live daemon's learned policy.
/// 3. `$XDG_STATE_HOME/xavier/xavier.runtime.json` (default
///    `~/.local/state/xavier/xavier.runtime.json`) — the XDG home for mutable
///    state on a normal node.
/// 4. Beside the config, still under a dot-directory, when there is no XDG
///    home at all.
pub fn resolve_runtime_state_path(config_path: &Path) -> PathBuf {
    if let Some(explicit) = std::env::var_os("XAVIER_RUNTIME_STATE_PATH") {
        if !explicit.is_empty() {
            return PathBuf::from(explicit);
        }
    }

    if std::env::var_os("XAVIER_CONFIG_PATH").is_some_and(|p| !p.is_empty()) {
        return state_path_beside(config_path);
    }

    if let Some(state_home) = dirs::state_dir() {
        return state_home.join("xavier").join(RUNTIME_STATE_FILENAME);
    }

    state_path_beside(config_path)
}

fn state_path_beside(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(RUNTIME_STATE_DIRNAME)
        .join(RUNTIME_STATE_FILENAME)
}

/// Save runtime state to the state dir. The versioned config file is never
/// written.
pub async fn save(settings: &XavierSettings) -> Result<()> {
    let path = resolve_runtime_state_path(&resolve_config_path());

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await.with_context(|| {
                format!(
                    "failed to create runtime state directory at {}",
                    parent.display()
                )
            })?;
        }
    }

    // D11: never persist credentials; they come from the vault / environment.
    // Round-trip through text, not `to_value`: f32 fields would otherwise widen
    // to f64 and persist as 0.29999998211860657 (float drift in the state).
    let mut document: Value = serde_json::from_str(
        &serde_json::to_string(settings).with_context(|| "failed to serialize settings to JSON")?,
    )
    .with_context(|| "failed to re-read serialized settings")?;
    strip_secrets(&mut document);
    let raw = serde_json::to_string_pretty(&document)
        .with_context(|| "failed to serialize settings to JSON")?;

    // Defence in depth: the state file is created 0600 (a pre-existing file
    // keeps its own mode, hence the permissions check below).
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .await
        .with_context(|| format!("failed to open runtime state file at {}", path.display()))?;
    tokio::io::AsyncWriteExt::write_all(&mut file, raw.as_bytes())
        .await
        .with_context(|| format!("failed to write runtime state at {}", path.display()))?;
    // tokio::fs::File buffers: without an explicit flush the tail of the
    // document can still be in the buffer when a caller reads the file back.
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .with_context(|| format!("failed to flush runtime state at {}", path.display()))?;
    tokio::io::AsyncWriteExt::shutdown(&mut file)
        .await
        .with_context(|| format!("failed to close runtime state at {}", path.display()))?;

    #[cfg(unix)]
    harden_state_file_permissions(&path)?;

    tracing::debug!("Xavier runtime state written to {}", path.display());
    Ok(())
}

/// Force 0600 on an existing state file (it may predate the mode we now pass).
#[cfg(unix)]
fn harden_state_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to stat runtime state at {}", path.display()))?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("failed to restrict permissions on {}", path.display()))?;
    }
    Ok(())
}

/// A rejected runtime state must never take the process down (D10).
#[cfg(test)]
mod tests {
    use super::*;
    // TempEnv holds ENV_LOCK for the whole test, which is what keeps these from
    // racing each other and the other settings tests: XAVIER_CONFIG_PATH is
    // process-wide, and cargo runs #[test] fns on parallel threads.
    use crate::settings::tests::TempEnv;

    /// Pins the defaults file and the state file into a scratch dir, and restores
    /// both the process env and the dir on drop. `None` defaults = the clean
    /// machine, where no config file exists at all.
    struct Fixture {
        dir: PathBuf,
        _env: TempEnv,
    }

    impl Fixture {
        fn new(name: &str, defaults: Option<&str>, state_json: &str) -> Self {
            // Snapshot the env BEFORE pinning anything, so Drop can put it back.
            let env = TempEnv::new();
            let dir =
                std::env::temp_dir().join(format!("xavier-d10-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");

            let config = dir.join("xavier.config.json");
            if let Some(contents) = defaults {
                std::fs::write(&config, contents).expect("write defaults");
            }
            let state_path = dir.join(RUNTIME_STATE_FILENAME);
            std::fs::write(&state_path, state_json).expect("write state");
            std::env::set_var("XAVIER_CONFIG_PATH", &config);
            std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &state_path);

            Self { dir, _env: env }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn load_survives_a_schema_incompatible_state_file() {
        let _f = Fixture::new(
            "incompatible",
            // Defaults pin a host/port that must survive the rejected overlay.
            Some(r#"{"server":{"host":"10.0.0.5","port":9999}}"#),
            // Valid JSON, but `server.port` is a string: exactly what a binary
            // that changed a field's type leaves behind on disk.
            r#"{"server":{"port":"not-a-number"}}"#,
        );

        let loaded = load()
            .expect("load must not fail on an incompatible state file")
            .expect("defaults exist, so settings must be Some");

        assert_eq!(loaded.server.host, "10.0.0.5", "defaults must be kept");
        assert_eq!(loaded.server.port, 9999, "defaults must be kept");
    }

    #[test]
    fn load_ignores_secrets_in_a_legacy_state_file() {
        let _f = Fixture::new(
            "legacy-secrets",
            Some(r#"{"server":{"port":9999}}"#),
            r#"{"server":{"port":7777},"security":{"token_secret":"s3cr3t"},
                "embedding":{"api_key":"k"},"pgheart":{"token":"t"}}"#,
        );
        let loaded = load().expect("load").expect("settings");
        assert_eq!(loaded.server.port, 7777, "non-secret state still applies");
        assert!(loaded.security.token_secret.is_none());
        assert!(loaded.embedding.api_key.is_none());
        assert!(loaded.pgheart.token.is_none());
    }

    #[test]
    fn strip_secrets_removes_nested_and_listed_credentials() {
        let mut v = serde_json::json!({
            "models": {"llm_api_key": "a", "provider": "local", "max_tokens": 5},
            "workspace": {"mini_experts": [{"name": "x", "api_key": "k"}]},
            "telegram": {"bot_token": "b"}
        });
        strip_secrets(&mut v);
        assert_eq!(
            v,
            serde_json::json!({
                "models": {"provider": "local", "max_tokens": 5},
                "workspace": {"mini_experts": [{"name": "x"}]},
                "telegram": {}
            })
        );
    }

    #[test]
    fn load_survives_an_unparseable_state_file() {
        let _f = Fixture::new(
            "unparseable",
            Some(r#"{"server":{"port":9999}}"#),
            "{ this is not json at all",
        );

        let loaded = load()
            .expect("load must not fail on an unparseable state file")
            .expect("defaults exist, so settings must be Some");
        assert_eq!(loaded.server.port, 9999);
    }

    #[test]
    fn load_still_applies_a_valid_state_overlay() {
        let _f = Fixture::new(
            "valid",
            Some(r#"{"server":{"host":"10.0.0.5","port":9999}}"#),
            r#"{"server":{"port":7777}}"#,
        );

        let loaded = load().expect("load").expect("settings");
        assert_eq!(
            loaded.server.host, "10.0.0.5",
            "untouched key keeps default"
        );
        assert_eq!(loaded.server.port, 7777, "overlay wins");
    }

    #[test]
    fn load_without_defaults_survives_an_incompatible_state_file() {
        // Clean machine: no defaults file, so a bad state must degrade to "no
        // settings" instead of erroring the caller out.
        let _f = Fixture::new("nodefaults", None, r#"{"server":{"port":{}}}"#);

        let loaded = load().expect("load must not fail without defaults");
        assert!(loaded.is_none(), "nothing to load is not an error");
    }
}
