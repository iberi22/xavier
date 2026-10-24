//! Explanation model backed by its own provider configuration.
//!
//! The explain route reads its LLM settings from `XAVIER_EXPLAIN_*` variables so
//! that a hosted model used only for explanations stays out of the daemon's
//! global fallback chain (chat, reflection, curation and the system1/2 agents).

use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Result};
use futures_util::future::BoxFuture;

use super::render::ExplanationModel;
use crate::agents::provider::client::ModelProviderClient;
use crate::agents::provider::config::DEFAULT_ANTHROPIC_BASE_URL;
use crate::agents::provider::types::{ApiFlavor, ProviderMode, ProviderTarget};
use crate::agents::provider::ModelProviderConfig;

const EXPLAIN_PROVIDER_ENV: &str = "XAVIER_EXPLAIN_PROVIDER";
const EXPLAIN_MODEL_ENV: &str = "XAVIER_EXPLAIN_MODEL";
const EXPLAIN_KEY_FILE_ENV: &str = "XAVIER_EXPLAIN_API_KEY_FILE";

/// Sends explanation prompts through a [`ModelProviderClient`].
pub struct ProviderExplanationModel {
    client: ModelProviderClient,
}

impl ProviderExplanationModel {
    pub fn new(client: ModelProviderClient) -> Self {
        Self { client }
    }

    /// Builds the explanation model from the explain-only environment.
    ///
    /// Without `XAVIER_EXPLAIN_PROVIDER` this is the global provider client. With it,
    /// the model comes from `XAVIER_EXPLAIN_MODEL` and the key from the file named by
    /// `XAVIER_EXPLAIN_API_KEY_FILE`. A missing or unsafe key file leaves the model
    /// unconfigured instead of falling back to the global chain.
    pub fn from_explain_env() -> Self {
        let Some(provider) = nonempty_env(EXPLAIN_PROVIDER_ENV) else {
            return Self::new(ModelProviderClient::from_env());
        };
        let config = explain_config(&provider);
        Self::new(ModelProviderClient::new(config))
    }
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Builds the explain-only configuration for `provider`. Performs no network I/O.
fn explain_config(provider: &str) -> ModelProviderConfig {
    if !provider.trim().eq_ignore_ascii_case("anthropic") {
        tracing::warn!(
            provider = %provider,
            "XAVIER_EXPLAIN_PROVIDER names a provider that is not supported for explanations; explain model not configured"
        );
        return ModelProviderConfig::disabled();
    }

    let api_key =
        nonempty_env(EXPLAIN_KEY_FILE_ENV).and_then(|path| read_explain_key(Path::new(&path)));

    ModelProviderConfig {
        provider_mode: ProviderMode::Cloud,
        api_flavor: ApiFlavor::AnthropicCompatible,
        provider_label: "anthropic".to_string(),
        model: nonempty_env(EXPLAIN_MODEL_ENV).unwrap_or_default(),
        api_key,
        base_url: Some(DEFAULT_ANTHROPIC_BASE_URL.to_string()),
        target: ProviderTarget::AnthropicMessages,
        lease_config: None,
        secret_injection_strategy: None,
        lease_token: None,
    }
}

/// Reads and trims a key file that only its owner can access.
///
/// Warnings name the file, never its contents. The permission check runs on the
/// open handle, so the file cannot be swapped between the check and the read.
fn read_explain_key(path: &Path) -> Option<String> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) => {
            tracing::warn!(
                file = %path.display(),
                error = ?err.kind(),
                "XAVIER_EXPLAIN_API_KEY_FILE cannot be opened; explain model not configured"
            );
            return None;
        }
    };
    let owner_only = file
        .metadata()
        .map(|metadata| is_owner_only(&metadata))
        .unwrap_or(false);
    if !owner_only {
        tracing::warn!(
            file = %path.display(),
            "XAVIER_EXPLAIN_API_KEY_FILE is group or world accessible (expected mode 0600); explain model not configured"
        );
        return None;
    }

    let mut text = String::new();
    if let Err(err) = file.read_to_string(&mut text) {
        tracing::warn!(
            file = %path.display(),
            error = ?err.kind(),
            "XAVIER_EXPLAIN_API_KEY_FILE cannot be read; explain model not configured"
        );
        return None;
    }
    let key = text.trim();
    if key.is_empty() {
        tracing::warn!(
            file = %path.display(),
            "XAVIER_EXPLAIN_API_KEY_FILE is empty; explain model not configured"
        );
        return None;
    }
    Some(key.to_string())
}

#[cfg(unix)]
fn is_owner_only(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o077 == 0
}

/// Without unix permission bits the file cannot be verified, so it is refused.
#[cfg(not(unix))]
fn is_owner_only(_metadata: &std::fs::Metadata) -> bool {
    false
}

/// Removes the API key and lease token from a provider error message.
fn redact(message: String, config: &ModelProviderConfig) -> String {
    [config.api_key.as_deref(), config.lease_token.as_deref()]
        .into_iter()
        .flatten()
        .filter(|secret| !secret.is_empty())
        .fold(message, |text, secret| text.replace(secret, "<redacted>"))
}

impl ExplanationModel for ProviderExplanationModel {
    fn complete<'a>(&'a self, system: &'a str, user: &'a str) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            match self.client.generate_text(system, user).await {
                Ok(response) => Ok(response.text),
                Err(err) => {
                    let message = format!(
                        "{} explanation request failed: {err:#}",
                        self.client.config.provider_label
                    );
                    Err(anyhow!(redact(message, &self.client.config)))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const SECRET: &str = "sk-test-dummy";

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Sets process env vars for one test and restores the previous values on drop.
    struct EnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(vars: &[(&'static str, Option<String>)]) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut saved = Vec::new();
            for (key, value) in vars {
                saved.push((*key, std::env::var(key).ok()));
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
            Self { _lock: lock, saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.saved {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    /// Env for one test: the global provider is pinned to `disabled` and the
    /// settings file points at a missing path inside `dir`.
    fn explain_env(
        dir: &std::path::Path,
        provider: Option<&str>,
        model: Option<&str>,
        key_file: Option<&std::path::Path>,
    ) -> Vec<(&'static str, Option<String>)> {
        vec![
            ("XAVIER_MODEL_PROVIDER", Some("disabled".to_string())),
            (
                "XAVIER_CONFIG_PATH",
                Some(dir.join("missing-config.json").display().to_string()),
            ),
            (EXPLAIN_PROVIDER_ENV, provider.map(str::to_string)),
            (EXPLAIN_MODEL_ENV, model.map(str::to_string)),
            (
                EXPLAIN_KEY_FILE_ENV,
                key_file.map(|path| path.display().to_string()),
            ),
        ]
    }

    #[cfg(unix)]
    fn write_key_file(dir: &std::path::Path, contents: &str, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("explain.key");
        std::fs::write(&path, contents).expect("write key file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("set key file mode");
        path
    }

    #[cfg(unix)]
    #[test]
    fn explain_provider_builds_anthropic_config_from_0600_key_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let key_file = write_key_file(dir.path(), &format!("  {SECRET}\n"), 0o600);
        let _env = EnvGuard::new(&explain_env(
            dir.path(),
            Some("anthropic"),
            Some("claude-haiku-5-5"),
            Some(key_file.as_path()),
        ));

        let model = ProviderExplanationModel::from_explain_env();
        let config = &model.client.config;

        assert_eq!(config.provider_label, "anthropic");
        assert_eq!(config.model, "claude-haiku-5-5");
        assert_eq!(config.api_key.as_deref(), Some(SECRET));
        assert_eq!(config.base_url.as_deref(), Some(DEFAULT_ANTHROPIC_BASE_URL));
        assert!(config.is_configured());
        assert!(!format!("{config:?}").contains(SECRET));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn explain_key_file_readable_by_others_is_not_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let key_file = write_key_file(dir.path(), SECRET, 0o644);
        let _env = EnvGuard::new(&explain_env(
            dir.path(),
            Some("anthropic"),
            Some("claude-haiku-5-5"),
            Some(key_file.as_path()),
        ));

        let model = ProviderExplanationModel::from_explain_env();

        assert!(model.client.config.api_key.is_none());
        assert!(!model.client.config.is_configured());
        // The rejected key must not surface in the error either. Unconfigured
        // clients fail before any request is sent, so this makes no network call.
        let err = model
            .complete("system", "user")
            .await
            .expect_err("unconfigured explain model must fail");
        assert!(!format!("{err:#}").contains(SECRET), "key leaked: {err:#}");
    }

    #[test]
    fn explain_without_provider_uses_global_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvGuard::new(&explain_env(
            dir.path(),
            None,
            Some("explain-model-unused"),
            None,
        ));

        let model = ProviderExplanationModel::from_explain_env();

        assert_eq!(model.client.config.provider_mode, ProviderMode::Disabled);
        assert_ne!(model.client.config.model, "explain-model-unused");
    }

    #[test]
    fn explain_unsupported_provider_is_not_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvGuard::new(&explain_env(
            dir.path(),
            Some("openai"),
            Some("gpt-explain-test"),
            None,
        ));

        let model = ProviderExplanationModel::from_explain_env();

        assert!(!model.client.config.is_configured());
        assert!(model.client.config.api_key.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn explain_anthropic_without_model_is_not_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let key_file = write_key_file(dir.path(), SECRET, 0o600);
        let _env = EnvGuard::new(&explain_env(
            dir.path(),
            Some("anthropic"),
            None,
            Some(key_file.as_path()),
        ));

        let model = ProviderExplanationModel::from_explain_env();

        assert!(!model.client.config.is_configured());
    }

    #[tokio::test]
    async fn unreachable_provider_returns_error_without_secret() {
        // Port 9 on loopback is closed, so the request fails without leaving the machine.
        let config = ModelProviderConfig::from_label("openai")
            .with_api_key(Some(SECRET.to_string()))
            .with_base_url(Some("http://127.0.0.1:9/v1".to_string()));
        let model = ProviderExplanationModel::new(ModelProviderClient::new(config));

        let err = model
            .complete("system", "user")
            .await
            .expect_err("unreachable endpoint must fail");
        let message = format!("{err:#}");

        assert!(
            message.starts_with("openai explanation request failed"),
            "{message}"
        );
        assert!(!message.contains(SECRET), "secret leaked: {message}");
    }

    #[test]
    fn redact_removes_api_key_and_lease_token() {
        let mut config = ModelProviderConfig::from_label("openai");
        config.api_key = Some("key-1".to_string());
        config.lease_token = Some("lease-2".to_string());

        let out = redact("key-1 and lease-2 in url".to_string(), &config);

        assert_eq!(out, "<redacted> and <redacted> in url");
    }
}
