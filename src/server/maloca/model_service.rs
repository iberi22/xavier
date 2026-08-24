//! Managed ModelRouter service wrapper for Xavier Maloca.
//!
//! Provides thread-safe state management (`Arc<Mutex<ProviderRouter>>`),
//! local Ollama endpoint discovery, and Clavis token lease caching with log masking.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::agents::provider::router::{ProviderKind, ProviderRouter};
use crate::clavis::{mask_key, register_secret};

/// A cached token lease managed by Clavis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClavisTokenLease {
    pub token_id: String,
    pub token_value: String,
    pub leased_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl ClavisTokenLease {
    /// Checks if the leased token is currently expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Returns a masked string representation of the token value for logging.
    pub fn masked_value(&self) -> String {
        mask_key(&self.token_value)
    }
}

/// Service configuration for `ModelRouterService`.
#[derive(Debug, Clone)]
pub struct ModelServiceConfig {
    pub default_provider: ProviderKind,
    pub ollama_url: Option<String>,
    pub default_lease_ttl_secs: u64,
}

impl Default for ModelServiceConfig {
    fn default() -> Self {
        Self {
            default_provider: ProviderKind::Local,
            ollama_url: None,
            default_lease_ttl_secs: 3600,
        }
    }
}

/// Thread-safe wrapper for `ProviderRouter` with Clavis token lease caching
/// and local Ollama model discovery.
#[derive(Clone)]
pub struct ModelRouterService {
    router: Arc<Mutex<ProviderRouter>>,
    lease_cache: Arc<Mutex<HashMap<String, ClavisTokenLease>>>,
    config: ModelServiceConfig,
}

impl ModelRouterService {
    /// Creates a new `ModelRouterService` with default or custom configuration.
    pub fn new(config: ModelServiceConfig) -> Self {
        let router = ProviderRouter::new(config.default_provider);
        Self {
            router: Arc::new(Mutex::new(router)),
            lease_cache: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Returns the active provider kind.
    pub async fn current_provider(&self) -> ProviderKind {
        let router = self.router.lock().await;
        router.current_provider()
    }

    /// Switches active provider manually.
    pub async fn switch_provider(&self, provider: ProviderKind) -> Result<()> {
        let mut router = self.router.lock().await;
        router.switch_to(provider)?;
        info!("ModelRouterService switched provider to {:?}", provider);
        Ok(())
    }

    /// Configures fallback chain for the router.
    pub async fn set_fallback_chain(&self, providers: Vec<ProviderKind>) {
        let mut router = self.router.lock().await;
        router.set_fallback_chain(providers);
    }

    /// Triggers provider failure fallback.
    pub async fn on_provider_failure(&self) -> Option<ProviderKind> {
        let mut router = self.router.lock().await;
        router.on_provider_failure()
    }

    /// Discover local Ollama endpoint reachability and register endpoints if available.
    pub async fn discover_ollama(&self) -> bool {
        let reachable = ProviderRouter::is_ollama_reachable().await;
        if reachable {
            let ollama_url = self.config.ollama_url.clone().unwrap_or_else(|| {
                crate::agents::provider::config::DEFAULT_LOCAL_BASE_URL.replace("/v1", "")
            });
            let mut router = self.router.lock().await;
            router.set_local_endpoints(vec![ollama_url.clone()]);
            info!("Discovered local Ollama endpoint at {}", ollama_url);
        } else {
            info!("Local Ollama endpoint unreachable");
        }
        reachable
    }

    /// Leases or retrieves a cached Clavis token, registering the token secret for log masking.
    pub async fn lease_clavis_token(
        &self,
        token_id: &str,
        token_value: &str,
        ttl_secs: Option<u64>,
    ) -> ClavisTokenLease {
        // Register raw secret token globally so log outputs mask it automatically
        register_secret(token_value);

        let mut cache = self.lease_cache.lock().await;
        if let Some(lease) = cache.get(token_id) {
            if !lease.is_expired() {
                return lease.clone();
            }
        }

        let ttl = ttl_secs.unwrap_or(self.config.default_lease_ttl_secs);
        let now = Utc::now();
        let lease = ClavisTokenLease {
            token_id: token_id.to_string(),
            token_value: token_value.to_string(),
            leased_at: now,
            expires_at: now + Duration::seconds(ttl as i64),
        };

        info!(
            "Leased Clavis token '{}' (masked: {}, expires in {}s)",
            token_id,
            lease.masked_value(),
            ttl
        );

        cache.insert(token_id.to_string(), lease.clone());
        lease
    }

    /// Removes expired tokens from the lease cache.
    pub async fn prune_expired_leases(&self) -> usize {
        let mut cache = self.lease_cache.lock().await;
        let initial_len = cache.len();
        cache.retain(|_, lease| !lease.is_expired());
        initial_len - cache.len()
    }

    /// Gets active cached lease count.
    pub async fn cached_lease_count(&self) -> usize {
        let cache = self.lease_cache.lock().await;
        cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clavis::mask_log_message;

    #[tokio::test]
    async fn test_model_service_initialization_and_switch() {
        let service = ModelRouterService::new(ModelServiceConfig::default());
        assert_eq!(service.current_provider().await, ProviderKind::Local);

        service.switch_provider(ProviderKind::OpenAI).await.unwrap();
        assert_eq!(service.current_provider().await, ProviderKind::OpenAI);
    }

    #[tokio::test]
    async fn test_clavis_token_leasing_and_log_masking() {
        let service = ModelRouterService::new(ModelServiceConfig::default());
        let secret = "sk-clavis-super-secret-key-998877";
        let token_id = "test-token";

        let lease = service.lease_clavis_token(token_id, secret, Some(10)).await;

        assert_eq!(lease.token_id, token_id);
        assert_eq!(lease.token_value, secret);
        assert!(!lease.is_expired());
        assert_eq!(service.cached_lease_count().await, 1);

        // Verify global log masker redacts the secret token
        let raw_log = format!("Authenticating with {}", secret);
        let masked_log = mask_log_message(&raw_log);
        assert!(!masked_log.contains(secret));
        assert!(masked_log.contains(&lease.masked_value()));
    }

    #[tokio::test]
    async fn test_clavis_lease_cache_expiration() {
        let service = ModelRouterService::new(ModelServiceConfig::default());
        let secret = "clavis-expiring-token-123456";

        // Lease token with 0 second TTL
        let lease = service
            .lease_clavis_token("expiring", secret, Some(0))
            .await;
        // Small pause to cross expiration
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        assert!(lease.is_expired());

        let pruned = service.prune_expired_leases().await;
        assert_eq!(pruned, 1);
        assert_eq!(service.cached_lease_count().await, 0);
    }

    #[tokio::test]
    async fn test_thread_safe_concurrent_access() {
        let service = ModelRouterService::new(ModelServiceConfig::default());
        let service_clone = service.clone();

        let handle1 = tokio::spawn(async move {
            service_clone
                .lease_clavis_token("t1", "secret-token-one-12345", Some(60))
                .await;
        });

        let service_clone2 = service.clone();
        let handle2 = tokio::spawn(async move {
            service_clone2
                .switch_provider(ProviderKind::Anthropic)
                .await
                .unwrap();
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        assert_eq!(service.current_provider().await, ProviderKind::Anthropic);
        assert_eq!(service.cached_lease_count().await, 1);
    }
}
