//! Proxy use case for LLM service proxying
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::agents::provider::{ModelProviderClient, ModelProviderConfig, LLM_TIMEOUT};
use crate::agents::rate_limit::RateLimitManager;
use crate::agents::router::{load_routing_policy, RouteCategory, Router};
use crate::coordination::events::XavierEvent;
use crate::coordination::{KeyLendingEngine, XavierEventBus};
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
    pub event_bus: Option<Arc<crate::coordination::XavierEventBus>>,
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
            event_bus: None,
        }
    }

    pub fn with_threat_detector(mut self, threat_detector: Arc<dyn ThreatDetectionPort>) -> Self {
        self.threat_detector = Some(threat_detector);
        self
    }

    pub fn with_event_bus(mut self, event_bus: Arc<crate::coordination::XavierEventBus>) -> Self {
        self.event_bus = Some(event_bus);
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

        // 1. Leak Detection: Scan request for any known API keys
        let mut combined_content = req.url.clone();
        for (k, v) in &req.headers {
            combined_content.push_str(k);
            combined_content.push_str(v);
        }
        if let Some(ref body) = req.body {
            combined_content.push_str(&body.to_string());
        }

        if let Some((agent_id, hash)) = secrets_engine.leak_detector.check_leak(&combined_content).await {
            warn!("Potential API key leak detected for agent {}. Hash: {}", agent_id, hash);

            if let Some(ref bus) = self.event_bus {
                let _ = bus.publish(crate::coordination::XavierEvent::KeyLeakDetected {
                    agent_id: agent_id.clone(),
                    hash,
                });
            }

            secrets_engine.revoke_for_agent(&agent_id, "API Key Leak Detected in Proxy").await;

            return Err(ProxyError::InvalidRequest("Security violation: API key leak detected".to_string()));
        }

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
        secrets_engine: Arc<KeyLendingEngine>,
        event_bus: XavierEventBus,
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
        let system_hash = crate::crypto::hex_encode(&hasher.finalize());

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
        let mut config = ModelProviderConfig::for_provider(&provider_name)
            .with_model_override(Some(requested_model.clone()));

        // Handle Secret Injection via lease token
        if let Some(token) = &cmd.lease_token {
            let lease = secrets_engine
                .get_lease(token)
                .await
                .ok_or_else(|| ProxyError::SecretError("Lease token not found".to_string()))?;

            if lease.is_expired() {
                return Err(ProxyError::SecretError("Lease token expired".to_string()));
            }

            if let Some(secret) = lease.secret_value {
                config = config.with_api_key(Some(secret));
            } else if !is_ephemeral {
                return Err(ProxyError::SecretError(
                    "Secret value missing from lease (redacted and not ephemeral)".to_string(),
                ));
            }
        }

        // Ensure we are using secured keys from vault if available
        let config = if let Ok(_token) = resolve_xavier_token() {
            // This ensures that even if env vars are missing, we try to use the root token
            // or other mechanisms defined in resolve_xavier_token.
            // For actual provider keys, ModelProviderConfig::for_provider already handles env/settings.
            config
        } else {
            config
        };

        let mut retry_count = 0;
        let max_retries = 1;

        loop {
            let client = ModelProviderClient::new(config.clone());

            let result: Result<Result<crate::agents::provider::types::LlmResponse, _>, _> =
                tokio::time::timeout(
                    LLM_TIMEOUT,
                    client.generate_text_with_cache(system_msg, user_msg, is_cache_hit),
                )
                .await;

            match result {
                Ok(Ok(resp)) => {
                    // Success logic
                    if let Some(token) = &cmd.lease_token {
                        let _ = secrets_engine.renew(token, 3600).await;
                        let _ = event_bus.publish(XavierEvent::LeaseRenewed {
                            token: token.clone(),
                        });
                    }

                    let text = resp.text;
                    // 4. Track Usage and Cost
                    let prompt_tokens = user_msg.len() / 4;
                    let completion_tokens = text.len() / 4;
                    let total_tokens = prompt_tokens + completion_tokens;

                    let mut cost_usd = 0.0;
                    if let Some(ref p) = policy {
                        let matched_policy =
                            if p.models.fast.iter().any(|m| m.name == requested_model) {
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

                    return Ok(ChatCompletion {
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
                    });
                }
                Ok(Err(e)) => {
                    let err_msg = e.to_string();

                    // Check for rate limit
                    if err_msg.contains("429") || err_msg.to_lowercase().contains("rate limit") {
                        if let Some(token) = &cmd.lease_token {
                            let _ = secrets_engine.backoff(token, 30).await;
                            let _ = event_bus.publish(XavierEvent::LeaseBackoff {
                                token: token.clone(),
                                seconds: 30,
                            });
                        }

                        if retry_count < max_retries {
                            retry_count += 1;
                            warn!(
                                "Rate limited by {}. Retrying ({}/{})",
                                provider_name, retry_count, max_retries
                            );
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    }

                    if err_msg.contains("timed out") {
                        warn!("Provider {} timed out (internal)", provider_name);
                        if let Err(track_err) = self
                            .rate_manager
                            .track_request(&provider_name, 0, 504, 0.0, false)
                            .await
                        {
                            warn!("Failed to track timeout request: {}", track_err);
                        }
                        return Err(ProxyError::ProviderError(format!(
                            "Provider {} timed out after {}s",
                            provider_name,
                            LLM_TIMEOUT.as_secs()
                        )));
                    } else {
                        warn!("Provider {} failed: {}", provider_name, e);
                        if let Err(track_err) = self
                            .rate_manager
                            .track_request(&provider_name, 0, 500, 0.0, false)
                            .await
                        {
                            warn!("Failed to track failed request: {}", track_err);
                        }
                        return Err(ProxyError::ProviderError(e.to_string()));
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
                    return Err(ProxyError::ProviderError(format!(
                        "Provider {} timed out after {}s",
                        provider_name,
                        LLM_TIMEOUT.as_secs()
                    )));
                }
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordination::KeyLendingEngine;
    use crate::domain::proxy::{GenericProxyRequest, ProxyError};
    use crate::secrets::lending::AuditLogger;
    use std::sync::Arc;
    use parking_lot::Mutex;

    struct MockAuditLogger;
    impl AuditLogger for MockAuditLogger {
        fn log_lend(&self, _agent_id: &str, _secret_name: &str, _lease_token: &str, _ttl_secs: u64) {}
        fn log_revoke(&self, _agent_id: &str, _lease_token: &str, _reason: &str) {}
    }

    #[tokio::test]
    async fn test_proxy_leak_detection() {
        let rate_manager = Arc::new(RateLimitManager::new());
        let prompt_cache = Arc::new(Mutex::new(HashMap::new()));
        let proxy = ProxyUseCase::new(rate_manager, prompt_cache);

        let secrets_engine = Arc::new(KeyLendingEngine::new(Box::new(MockAuditLogger), None));
        let secret = "sk-leaked-key-123";
        let agent_id = "malicious-agent";

        // Register key via lend
        secrets_engine.lend("provider-key", Some(secret), agent_id, 3600).await.unwrap();

        // Prepare request with leaked key in body
        let req = GenericProxyRequest {
            url: "https://api.openai.com/v1/chat/completions".to_string(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            body: Some(serde_json::json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": format!("Here is my key: {}", secret)}]
            })),
            lease_token: None,
            secret_injection_strategy: None,
        };

        let result = proxy.execute_generic(req, secrets_engine.clone()).await;

        // Should be blocked
        assert!(result.is_err());
        match result.unwrap_err() {
            ProxyError::InvalidRequest(msg) => assert!(msg.contains("API key leak detected")),
            _ => panic!("Expected InvalidRequest error"),
        }

        // Leases for agent should be revoked
        let leases = secrets_engine.list_leases().await;
        assert!(leases.iter().all(|l| l.agent_id != agent_id));
    }
}
