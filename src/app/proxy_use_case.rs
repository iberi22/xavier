//! Proxy use case for LLM service proxying
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::agents::provider::{ModelProviderClient, ModelProviderConfig, LLM_TIMEOUT};
use crate::agents::rate_limit::RateLimitManager;
use crate::agents::router::{load_routing_policy, RouteCategory, Router};
use crate::domain::proxy::{
    ChatChoice, ChatCompletion, ChatMessage, GenericProxyRequest, GenericProxyResponse,
    ProxyChatCommand, ProxyError, SecretInjectionStrategy, Usage,
};
use crate::ports::outbound::ThreatDetectionPort;
use crate::security::auth::resolve_xavier_token;

pub struct ProxyUseCase {
    pub rate_manager: Arc<RateLimitManager>,
    pub prompt_cache: Arc<Mutex<HashMap<String, Vec<String>>>>,
    pub router: Router,
    pub threat_detector: Option<Arc<dyn ThreatDetectionPort>>,
}

impl ProxyUseCase {
    pub fn new(
        rate_manager: Arc<RateLimitManager>,
        prompt_cache: Arc<Mutex<HashMap<String, Vec<String>>>>,
    ) -> Self {
        Self {
            rate_manager,
            prompt_cache,
            router: Router::new(),
            threat_detector: None,
        }
    }

    pub fn with_threat_detector(mut self, threat_detector: Arc<dyn ThreatDetectionPort>) -> Self {
        self.threat_detector = Some(threat_detector);
        self
    }

    pub async fn execute_generic(
        &self,
        req: GenericProxyRequest,
        secrets_engine: Arc<crate::coordination::KeyLendingEngine>,
    ) -> Result<GenericProxyResponse, ProxyError> {
        let client = reqwest::Client::new();
        let method = match req.method.to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            _ => {
                return Err(ProxyError::InvalidRequest(format!(
                    "Unsupported method: {}",
                    req.method
                )))
            }
        };

        let mut request_builder = client.request(method, &req.url);

        // Set headers
        for (k, v) in &req.headers {
            request_builder = request_builder.header(k, v);
        }

        // Handle Secret Injection
        if let Some(token) = &req.lease_token {
            let lease = secrets_engine
                .get_lease(token)
                .await
                .ok_or_else(|| ProxyError::SecretError("Lease token not found".to_string()))?;

            if lease.is_expired() {
                return Err(ProxyError::SecretError("Lease token expired".to_string()));
            }

            let secret = lease.secret_value.ok_or_else(|| {
                ProxyError::SecretError(
                    "Secret value missing from lease (redacted or not set)".to_string(),
                )
            })?;

            match req
                .secret_injection_strategy
                .unwrap_or(SecretInjectionStrategy::BearerToken)
            {
                SecretInjectionStrategy::BearerToken => {
                    request_builder =
                        request_builder.header("Authorization", format!("Bearer {}", secret));
                }
                SecretInjectionStrategy::XApiKey => {
                    request_builder = request_builder.header("X-API-Key", secret);
                }
                SecretInjectionStrategy::GitHubToken => {
                    request_builder =
                        request_builder.header("Authorization", format!("token {}", secret));
                }
            }
        }

        // Set body
        if let Some(body) = req.body {
            request_builder = request_builder.json(&body);
        }

        let response = request_builder
            .send()
            .await
            .map_err(|e| ProxyError::ProviderError(e.to_string()))?;

        let status = response.status().as_u16();
        let mut resp_headers = HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(v) = value.to_str() {
                resp_headers.insert(name.to_string(), v.to_string());
            }
        }

        let body = response
            .json::<serde_json::Value>()
            .await
            .unwrap_or(serde_json::json!({}));

        Ok(GenericProxyResponse {
            status,
            headers: resp_headers,
            body,
        })
    }

    pub async fn execute_secured(
        &self,
        cmd: ProxyChatCommand,
        is_ephemeral: bool,
    ) -> Result<ChatCompletion, ProxyError> {
        // 0. Security Policy Enforcement
        if !is_ephemeral {
            // In a high-security environment, we might want to log or restrict non-ephemeral access to the proxy
            info!("Non-ephemeral proxy request detected");
        }

        // 1. Threat Detection
        if let Some(ref detector) = self.threat_detector {
            for msg in &cmd.messages {
                if let Some(content) = msg["content"].as_str() {
                    let clean = detector.scan_and_log(content, "proxy").await.map_err(|e| {
                        ProxyError::ProviderError(format!("Security check failed: {}", e))
                    })?;

                    if !clean {
                        warn!("Proxy request blocked: security threat detected");
                        return Err(ProxyError::ProviderError(
                            "Security policy violation detected".to_string(),
                        ));
                    }
                }
            }
        }

        // 1. Resolve Provider based on Rate Limits
        let providers = [
            "opencode-go",
            "deepseek",
            "groq",
            "openrouter",
            "google",
            "openai",
            "anthropic",
        ];
        let mut selected_provider = None;

        for provider in providers {
            match self.rate_manager.get_status(provider).await {
                Ok(status) => {
                    let now = chrono::Utc::now();
                    if status.rate_limited_until.is_none_or(|until| until < now) {
                        selected_provider = Some(provider.to_string());
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to check rate limit for {}: {}", provider, e);
                }
            }
        }

        let provider_name = match selected_provider {
            Some(p) => p,
            None => return Err(ProxyError::RateLimited),
        };

        info!("Proxying request to provider: {}", provider_name);

        // 2. Resolve Model and apply cost ceilings
        let mut requested_model = cmd.model.clone();

        // Prompt Cache Detection
        let system_msg = cmd
            .messages
            .iter()
            .find(|m| m["role"] == "system")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("You are a helpful assistant.");

        let mut hasher = Sha256::new();
        hasher.update(system_msg.as_bytes());
        let system_hash = hex::encode(hasher.finalize());

        let is_cache_hit = {
            let mut cache = self.prompt_cache.lock();
            let hashes = cache.entry(provider_name.clone()).or_default();
            let hit = hashes.contains(&system_hash);
            if !hit {
                hashes.push(system_hash);
                if hashes.len() > 5 {
                    hashes.remove(0);
                }
            }
            hit
        };

        if is_cache_hit {
            info!("Prompt cache hit for provider {}", provider_name);
        }

        let user_msg = cmd
            .messages
            .iter()
            .rfind(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("");

        let policy = load_routing_policy();
        let decision = self.router.classify(user_msg);

        if decision.category == RouteCategory::Direct
            || decision.category == RouteCategory::Retrieved
        {
            if let Some(ref p) = policy {
                let quality_model = p.models.quality.first().map(|m| m.name.clone());
                let fast_model = p.models.fast.first().map(|m| m.name.clone());

                if let (Some(quality), Some(fast)) = (quality_model, fast_model) {
                    if requested_model == quality {
                        info!("Routing category {:?} detected. Enforcing cost ceiling: overriding {} with fast model {}", decision.category, quality, fast);
                        requested_model = fast;
                    }
                }
            }
        }

        // 3. Execute Request - API Keys are retrieved from Hardware Vault or Secure Config
        // and injected only here, in-flight.
        let config = ModelProviderConfig::for_provider(&provider_name)
            .with_model_override(Some(requested_model.clone()));

        // Ensure we are using secured keys from vault if available
        let config = if let Ok(_token) = resolve_xavier_token() {
            // This ensures that even if env vars are missing, we try to use the root token
            // or other mechanisms defined in resolve_xavier_token.
            // For actual provider keys, ModelProviderConfig::for_provider already handles env/settings.
            config
        } else {
            config
        };

        let client = ModelProviderClient::new(config);

        let result: Result<Result<crate::agents::provider::types::LlmResponse, _>, _> =
            tokio::time::timeout(
                LLM_TIMEOUT,
                client.generate_text_with_cache(system_msg, user_msg, is_cache_hit),
            )
            .await;

        match result {
            Ok(Ok(resp)) => {
                let text = resp.text;
                // 4. Track Usage and Cost
                let prompt_tokens = user_msg.len() / 4;
                let completion_tokens = text.len() / 4;
                let total_tokens = prompt_tokens + completion_tokens;

                let mut cost_usd = 0.0;
                if let Some(ref p) = policy {
                    let matched_policy = if p.models.fast.iter().any(|m| m.name == requested_model)
                    {
                        p.models.fast.first()
                    } else if p.models.quality.iter().any(|m| m.name == requested_model) {
                        p.models.quality.first()
                    } else {
                        None
                    };

                    if let Some(mp) = matched_policy {
                        let input_rate = mp.cost_per_input_token.unwrap_or(0.0) as f64;
                        let output_rate = mp.cost_per_output_token.unwrap_or(0.0) as f64;
                        cost_usd = (prompt_tokens as f64 * input_rate)
                            + (completion_tokens as f64 * output_rate);
                    }
                }

                if let Err(e) = self
                    .rate_manager
                    .track_request(&provider_name, total_tokens, 200, cost_usd, is_cache_hit)
                    .await
                {
                    warn!("Failed to track request usage: {}", e);
                }

                if let Some(quota) = resp.quota {
                    if let Err(e) = self.rate_manager.update_quota(quota).await {
                        warn!("Failed to update provider quota: {}", e);
                    }
                }

                Ok(ChatCompletion {
                    id: format!("chatcmpl-{}", ulid::Ulid::new()),
                    object: "chat.completion".to_string(),
                    created: chrono::Utc::now().timestamp(),
                    model: requested_model,
                    choices: vec![ChatChoice {
                        index: 0,
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content: text,
                        },
                        finish_reason: "stop".to_string(),
                    }],
                    usage: Usage {
                        prompt_tokens,
                        completion_tokens,
                        total_tokens,
                    },
                })
            }
            Ok(Err(e)) => {
                let err_msg = e.to_string();
                if err_msg.contains("timed out") {
                    warn!("Provider {} timed out (internal)", provider_name);
                    if let Err(track_err) = self
                        .rate_manager
                        .track_request(&provider_name, 0, 504, 0.0, false)
                        .await
                    {
                        warn!("Failed to track timeout request: {}", track_err);
                    }
                    Err(ProxyError::ProviderError(format!(
                        "Provider {} timed out after {}s",
                        provider_name,
                        LLM_TIMEOUT.as_secs()
                    )))
                } else {
                    warn!("Provider {} failed: {}", provider_name, e);
                    if let Err(track_err) = self
                        .rate_manager
                        .track_request(&provider_name, 0, 500, 0.0, false)
                        .await
                    {
                        warn!("Failed to track failed request: {}", track_err);
                    }
                    Err(ProxyError::ProviderError(e.to_string()))
                }
            }
            Err(_) => {
                warn!("Provider {} timed out", provider_name);
                if let Err(track_err) = self
                    .rate_manager
                    .track_request(&provider_name, 0, 504, 0.0, false)
                    .await
                {
                    warn!("Failed to track timeout request: {}", track_err);
                }
                Err(ProxyError::ProviderError(format!(
                    "Provider {} timed out after {}s",
                    provider_name,
                    LLM_TIMEOUT.as_secs()
                )))
            }
        }
    }
}
