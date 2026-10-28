//! Scoped language-model configuration and bounded provider probes.
use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use reqwest::{Client, RequestBuilder, Response};
use serde_json::{json, Value};
use std::{io::Read, path::PathBuf, time::Duration};

use crate::{
    agents::provider::{types::ApiFlavor, ModelProviderConfig},
    secrets::vault::HardwareVault,
    settings::XavierSettings,
};

// Startup exports settings into env; retain provenance for those generated values.
static ORIGINAL_ENV: std::sync::OnceLock<std::collections::HashSet<String>> =
    std::sync::OnceLock::new();
pub fn capture_environment() {
    let _ = ORIGINAL_ENV.set(
        std::env::vars_os()
            .filter_map(|(key, value)| {
                if value.is_empty() {
                    None
                } else {
                    key.into_string().ok()
                }
            })
            .collect(),
    );
}

#[derive(Args, Debug, Clone)]
#[command(
    after_help = "Examples:\n  xavier llm status --json\n  xavier llm set --provider local --model llama3.2 --url http://127.0.0.1:11434/v1\n  xavier llm set --provider openai --model MODEL --api-key-file /run/secrets/key --scope explain\n  xavier llm test --json\n  xavier llm list-models --json"
)]
pub struct LlmArgs {
    #[command(subcommand)]
    pub command: LlmCommand,
    /// Emit one JSON document on stdout; diagnostics go to stderr
    #[arg(long, global = true)]
    pub json: bool,
    /// Select independent explanation settings, or the general provider
    #[arg(long, global = true, value_enum, default_value = "general")]
    pub scope: Scope,
    /// Per-request timeout in seconds (1–120)
    #[arg(long, global = true, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=120))]
    pub timeout_secs: u64,
}
#[derive(Subcommand, Debug, Clone)]
pub enum LlmCommand {
    /// Show effective settings, sources, key presence and daemon health
    Status,
    /// Save runtime settings; restart the daemon to use them
    #[command(
        after_help = "Example: xavier llm set --provider openai --model MODEL --api-key-stdin --json < /run/secrets/key"
    )]
    Set {
        #[arg(long, value_parser = ["local", "openai", "openrouter", "groq", "anthropic"])]
        provider: String,
        #[arg(long)]
        model: String,
        /// HTTP(S) API root, normally ending in /v1; no credentials or query
        #[arg(long)]
        url: Option<String>,
        /// Read a key and store it in the encrypted provider vault
        #[arg(long, conflicts_with = "api_key_stdin")]
        api_key_file: Option<PathBuf>,
        /// Read a key from stdin and store it in the encrypted provider vault
        #[arg(long)]
        api_key_stdin: bool,
    },
    /// Probe /models and a one-token completion without printing content
    Test,
    /// List model IDs from the provider's /models endpoint
    ListModels,
}
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    General,
    Explain,
}
impl Scope {
    fn label(self) -> &'static str {
        if self == Self::General {
            "general"
        } else {
            "explain"
        }
    }
}
struct Failure {
    code: i32,
    message: String,
}
type Outcome<T> = std::result::Result<T, Failure>;
fn fail(code: i32, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into(),
    }
}

pub async fn handle(args: &LlmArgs) -> Result<()> {
    match run(args).await {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value)?);
            if value["restart_required"] == true {
                eprintln!("Restart required: restart the Xavier daemon; environment overrides still take precedence.");
            }
            Ok(())
        }
        Err(error) => {
            if args.json {
                println!(
                    "{}",
                    json!({"schema_version":1,"ok":false,"error":error.message})
                );
            }
            eprintln!("{}", error.message);
            std::process::exit(error.code);
        }
    }
}
pub async fn setup_local(model: String, json: bool) -> Result<()> {
    handle(&LlmArgs {
        command: LlmCommand::Set {
            provider: "local".into(),
            model,
            url: None,
            api_key_file: None,
            api_key_stdin: false,
        },
        json,
        scope: Scope::General,
        timeout_secs: 10,
    })
    .await
}
async fn run(args: &LlmArgs) -> Outcome<Value> {
    if let LlmCommand::Set {
        provider,
        model,
        url,
        api_key_file,
        api_key_stdin,
    } = &args.command
    {
        return save(
            args.scope,
            provider,
            model,
            url.as_deref(),
            api_key_file.as_ref(),
            *api_key_stdin,
        )
        .await;
    }
    let config = if args.scope == Scope::Explain {
        crate::explain::model::resolved_config()
    } else {
        ModelProviderConfig::from_env()
    };
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout_secs))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| fail(1, "Could not initialize HTTP client"))?;
    if matches!(args.command, LlmCommand::Status) {
        return status(args.scope, &config, args.timeout_secs).await;
    }
    let base = checked_url(config.base_url.as_deref().unwrap_or(""))?;
    let models = list_models(&client, &config, &base).await?;
    if matches!(args.command, LlmCommand::ListModels) {
        return Ok(
            json!({"schema_version":1,"ok":true,"scope":args.scope.label(),"provider":config.provider_label,"models":models}),
        );
    }
    if config.model.trim().is_empty() {
        return Err(fail(
            2,
            "No model configured; run xavier llm set --provider PROVIDER --model ID",
        ));
    }
    let anthropic = matches!(config.api_flavor, ApiFlavor::AnthropicCompatible);
    let endpoint = if anthropic {
        "messages"
    } else {
        "chat/completions"
    };
    let body =
        json!({"model":config.model,"max_tokens":1,"messages":[{"role":"user","content":"Hi"}]});
    let response =
        send(auth(client.post(format!("{base}/{endpoint}")), &config).json(&body)).await?;
    let value: Value = response
        .json()
        .await
        .map_err(|e| body_failure(e, "Provider returned invalid completion JSON"))?;
    let valid = if anthropic {
        value["content"].as_array().is_some_and(|v| !v.is_empty())
    } else {
        value["choices"][0]["message"].is_object()
    };
    if !valid || value.get("error").is_some() {
        return Err(fail(1, "Provider returned an invalid completion response"));
    }
    Ok(
        json!({"schema_version":1,"ok":true,"scope":args.scope.label(),"provider":config.provider_label,"model":config.model,"models_ok":true,"completion_ok":true}),
    )
}
fn checked_url(raw: &str) -> Outcome<String> {
    let url = reqwest::Url::parse(raw).map_err(|_| {
        fail(
            2,
            "Provider URL must be an HTTP(S) API root without credentials, query or fragment",
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(fail(
            2,
            "Provider URL must be an HTTP(S) API root without credentials, query or fragment",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}
fn auth(request: RequestBuilder, config: &ModelProviderConfig) -> RequestBuilder {
    if matches!(config.api_flavor, ApiFlavor::AnthropicCompatible) {
        let request = request.header("anthropic-version", "2023-06-01");
        match &config.api_key {
            Some(key) => request.header("x-api-key", key),
            None => request,
        }
    } else {
        match &config.api_key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }
}
fn body_failure(error: reqwest::Error, message: &str) -> Failure {
    if error.is_timeout() || error.is_connect() {
        fail(4, "Provider response timed out or disconnected")
    } else {
        fail(1, message)
    }
}
async fn send(request: RequestBuilder) -> Outcome<Response> {
    let response = request.send().await.map_err(|e| {
        fail(
            if e.is_connect() || e.is_timeout() {
                4
            } else {
                1
            },
            "Provider connection failed or timed out; check its URL and availability",
        )
    })?;
    let status = response.status();
    if status.is_success() {
        Ok(response)
    } else {
        Err(fail(
            if matches!(status.as_u16(), 401 | 403) {
                3
            } else {
                1
            },
            format!("Provider request failed (HTTP {})", status.as_u16()),
        ))
    }
}
async fn list_models(
    client: &Client,
    config: &ModelProviderConfig,
    base: &str,
) -> Outcome<Vec<String>> {
    let response = send(auth(client.get(format!("{base}/models")), config)).await?;
    let value: Value = response
        .json()
        .await
        .map_err(|e| body_failure(e, "Provider returned invalid model-list JSON"))?;
    let data = value["data"]
        .as_array()
        .ok_or_else(|| fail(1, "Provider model list must contain a data array"))?;
    data.iter()
        .map(|entry| {
            entry["id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| fail(1, "Provider model entry is missing an ID"))
        })
        .collect()
}
fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}
fn key_name(provider: &str, scope: Scope) -> String {
    if scope == Scope::Explain {
        format!("EXPLAIN_{}_API_KEY", provider.to_ascii_uppercase())
    } else if provider == "local" {
        "XAVIER_LOCAL_LLM_API_KEY".into()
    } else {
        format!("{}_API_KEY", provider.to_ascii_uppercase())
    }
}
async fn save(
    scope: Scope,
    provider: &str,
    model: &str,
    url: Option<&str>,
    file: Option<&PathBuf>,
    stdin: bool,
) -> Outcome<Value> {
    if model.trim().is_empty() {
        return Err(fail(2, "A nonempty --model ID is required"));
    }
    let url = url.map(checked_url).transpose()?;
    let mut settings = XavierSettings::load()
        .map_err(|_| fail(1, "Could not load settings"))?
        .unwrap_or_default();
    let key = if file.is_some() || stdin {
        let mut value = zeroize::Zeroizing::new(String::new());
        let reader: Box<dyn Read> = match file {
            Some(path) => Box::new(
                std::fs::File::open(path).map_err(|_| fail(2, "Could not open --api-key-file"))?,
            ),
            None => Box::new(std::io::stdin()),
        };
        reader
            .take(65537)
            .read_to_string(&mut value)
            .map_err(|_| fail(2, "Could not read API key input"))?;
        if value.len() > 65536 || value.trim().is_empty() {
            return Err(fail(2, "API key input must be nonempty and at most 64 KiB"));
        }
        Some(value)
    } else {
        None
    };
    if scope == Scope::Explain {
        if settings.models.explain_provider.as_deref() != Some(provider) && url.is_none() {
            settings.models.explain_url = None;
        }
        settings.models.explain_provider = Some(provider.into());
        settings.models.explain_model = Some(model.trim().into());
        if let Some(url) = url {
            settings.models.explain_url = Some(url);
        }
    } else {
        if settings.models.provider != provider && url.is_none() {
            settings.models.cloud_llm_url = None;
        }
        settings.models.provider = provider.into();
        if provider == "local" {
            settings.models.local_llm_model = model.trim().into();
            if let Some(url) = url {
                settings.models.local_llm_url = url;
            }
        } else {
            settings.models.llm_model = Some(model.trim().into());
            if provider == "openrouter" {
                settings.models.cloud_llm_model = Some(model.trim().into());
            }
            if let Some(url) = url {
                settings.models.cloud_llm_url = Some(url);
            }
        }
    }
    if let Some(key) = key {
        HardwareVault::new("xavier").store_secret(&key_name(provider, scope), key.trim()).map_err(|_| {
            let hint = if scope == Scope::Explain { "XAVIER_EXPLAIN_API_KEY_FILE".into() } else { key_name(provider, scope) };
            fail(1, format!("Could not securely persist the API key; configure {hint} in the daemon environment"))
        })?;
    }
    settings
        .save()
        .await
        .map_err(|_| fail(1, "Could not save runtime settings"))?;
    Ok(
        json!({"schema_version":1,"ok":true,"scope":scope.label(),"provider":provider,"model":model.trim(),"restart_required":true}),
    )
}

type SourceFields = (
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);

struct Sources {
    config: Value,
    runtime: Value,
}
impl Sources {
    fn new() -> Self {
        fn read(path: PathBuf) -> Value {
            std::fs::read(path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or(Value::Null)
        }
        Self {
            config: read(XavierSettings::resolve_config_path()),
            runtime: read(XavierSettings::resolve_runtime_state_path()),
        }
    }
    fn field(&self, names: &[&str]) -> String {
        for name in names {
            let pointer = format!("/models/{name}");
            match self.runtime.pointer(&pointer) {
                Some(value) => {
                    if value.as_str().is_some_and(|v| !v.trim().is_empty()) {
                        return "runtime".into();
                    }
                }
                None => {
                    if self
                        .config
                        .pointer(&pointer)
                        .and_then(Value::as_str)
                        .is_some_and(|v| !v.trim().is_empty())
                    {
                        return "config".into();
                    }
                }
            }
        }
        "default".into()
    }
    fn source(&self, env: &[&str], fields: &[&str]) -> String {
        for name in env {
            if nonempty_env(name).is_none() {
                continue;
            }
            if ORIGINAL_ENV
                .get()
                .is_some_and(|names| !names.contains(*name))
            {
                let generated = match *name {
                    "XAVIER_MODEL_PROVIDER" => {
                        if nonempty_env("XAVIER_PROVIDER").is_some() {
                            return "env:XAVIER_PROVIDER".into();
                        }
                        Some("provider")
                    }
                    "XAVIER_LOCAL_LLM_MODEL" => Some("local_llm_model"),
                    "XAVIER_LLM_MODEL" => Some("llm_model"),
                    _ => None,
                };
                if let Some(field) = generated {
                    return self.field(&[field]);
                }
            }
            return format!("env:{name}");
        }
        self.field(fields)
    }
}
async fn status(scope: Scope, cfg: &ModelProviderConfig, timeout: u64) -> Outcome<Value> {
    let sources = Sources::new();
    let settings = XavierSettings::load().ok().flatten().unwrap_or_default();
    let explicit_explain = scope == Scope::Explain
        && (nonempty_env("XAVIER_EXPLAIN_PROVIDER").is_some()
            || settings
                .models
                .explain_provider
                .as_ref()
                .is_some_and(|provider| !provider.trim().is_empty()));
    let provider = cfg.provider_label.as_str();
    let (provider_source, model_source, url_source, key_source) = if explicit_explain {
        let key_source = if nonempty_env("XAVIER_EXPLAIN_API_KEY_FILE").is_some() {
            "env:XAVIER_EXPLAIN_API_KEY_FILE"
        } else if cfg.api_key.is_some() {
            "vault"
        } else {
            "unset"
        };
        (
            sources.source(&["XAVIER_EXPLAIN_PROVIDER"], &["explain_provider"]),
            sources.source(&["XAVIER_EXPLAIN_MODEL"], &["explain_model"]),
            sources.field(&["explain_url"]),
            key_source.to_string(),
        )
    } else {
        let (model_env, model_fields, url_env, url_fields, key_env): SourceFields = match provider {
            "local" if matches!(cfg.api_flavor, ApiFlavor::AnthropicCompatible) => (
                &["XAVIER_LOCAL_LLM_MODEL", "XAVIER_LLM_MODEL"],
                &["local_llm_model", "llm_model"],
                &["XAVIER_LOCAL_ANTHROPIC_URL", "XAVIER_LOCAL_LLM_URL"],
                &["local_anthropic_url", "local_llm_url"],
                &["XAVIER_LOCAL_LLM_API_KEY"],
            ),
            "local" => (
                &["XAVIER_LOCAL_LLM_MODEL", "XAVIER_LLM_MODEL"],
                &["local_llm_model", "llm_model"],
                &["XAVIER_LOCAL_LLM_URL"],
                &["local_llm_url"],
                &["XAVIER_LOCAL_LLM_API_KEY"],
            ),
            "openrouter" => (
                &[
                    "XAVIER_OPENROUTER_MODEL",
                    "XAVIER_LLM_MODEL",
                    "OPENROUTER_MODEL",
                ],
                &["cloud_llm_model", "llm_model"],
                &["XAVIER_OPENROUTER_URL", "OPENROUTER_BASE_URL"],
                &["cloud_llm_url"],
                &["XAVIER_OPENROUTER_API_KEY", "OPENROUTER_API_KEY"],
            ),
            "groq" => (
                &["XAVIER_LLM_MODEL", "GROQ_MODEL"],
                &["llm_model"],
                &["GROQ_BASE_URL"],
                &["cloud_llm_url"],
                &["GROQ_API_KEY"],
            ),
            "anthropic" => (
                &["XAVIER_LLM_MODEL", "ANTHROPIC_MODEL"],
                &["llm_model"],
                &["ANTHROPIC_BASE_URL", "XAVIER_CLOUD_LLM_URL"],
                &["cloud_llm_url"],
                &["ANTHROPIC_API_KEY"],
            ),
            _ => (
                &["XAVIER_LLM_MODEL", "OPENAI_MODEL"],
                &["llm_model"],
                &["OPENAI_BASE_URL"],
                &["cloud_llm_url"],
                &["OPENAI_API_KEY"],
            ),
        };
        let key_source =
            if let Some(name) = key_env.iter().find(|name| nonempty_env(name).is_some()) {
                format!("env:{name}")
            } else if provider == "local"
                && settings
                    .models
                    .local_llm_api_key
                    .as_ref()
                    .is_some_and(|key| !key.trim().is_empty())
            {
                sources.source(&["XAVIER_LOCAL_LLM_API_KEY"], &["local_llm_api_key"])
            } else if HardwareVault::new("xavier")
                .get_secret(&key_name(provider, Scope::General))
                .ok()
                .is_some_and(|v| !v.trim().is_empty())
            {
                "vault".into()
            } else if cfg.api_key.is_some() && provider != "local" {
                sources.source(&["XAVIER_LLM_API_KEY"], &["llm_api_key"])
            } else {
                "unset".into()
            };
        let provider_source = sources.source(
            &[
                "XAVIER_MODEL_PROVIDER",
                "XAVIER_PROVIDER",
                "XAVIER_OPENROUTER_API_KEY",
                "OPENROUTER_API_KEY",
            ],
            &["provider"],
        );
        (
            provider_source,
            sources.source(model_env, model_fields),
            sources.source(url_env, url_fields),
            key_source,
        )
    };
    let health_client = Client::builder()
        .timeout(Duration::from_secs(timeout.min(2)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| fail(1, "Could not initialize HTTP client"))?;
    let health_url = format!(
        "{}/health",
        crate::cli::config::resolve_base_url().trim_end_matches('/')
    );
    let mut request = health_client.get(health_url);
    if let Some(token) = nonempty_env("XAVIER_TOKEN") {
        request = request.header("X-Xavier-Token", token);
    }
    let daemon = match request.send().await {
        Ok(response) => {
            let http_status = response.status().as_u16();
            let body = response.json::<Value>().await.ok();
            json!({"reachable":true,"llm_configured":body.as_ref().and_then(|b| b["llm"]["configured"].as_bool()),"http_status":http_status})
        }
        Err(_) => json!({"reachable":false,"llm_configured":null,"http_status":null}),
    };
    // URL fields may have been supplied by older config files; never echo credentials.
    let safe_url = cfg
        .base_url
        .as_deref()
        .and_then(|raw| checked_url(raw).ok());
    let key_set = key_source != "unset"
        && cfg
            .api_key
            .as_ref()
            .is_some_and(|key| !key.trim().is_empty());
    Ok(
        json!({"schema_version":1,"ok":true,"scope":scope.label(),"provider":provider,"model":cfg.model,"base_url":safe_url,"api_key":if key_set {"set"} else {"unset"},"source":{"provider":provider_source,"model":model_source,"base_url":url_source,"api_key":key_source},"daemon":daemon}),
    )
}
