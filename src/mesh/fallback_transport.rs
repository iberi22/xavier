//! FallbackMeshTransport — libp2p→http→supabase realtime (WAVE-1.03)
//!
//! Tries mesh transports in order until one succeeds.

use anyhow::Result;

/// Trait for mesh transport backends
#[async_trait::async_trait]
pub trait FallbackTransport: Send + Sync {
    async fn send(&self, peer_id: &str, payload: &[u8]) -> Result<()>;
    async fn is_available(&self) -> bool;
    fn name(&self) -> &'static str;
}

/// Fallback chain: libp2p → http → supabase realtime
pub struct FallbackMeshTransport {
    transports: Vec<std::sync::Arc<dyn FallbackTransport>>,
}

impl FallbackMeshTransport {
    pub fn new(transports: Vec<std::sync::Arc<dyn FallbackTransport>>) -> Self {
        Self { transports }
    }

    pub fn chain_from_env() -> Vec<String> {
        let chain = std::env::var("XAVIER_MESH_FALLBACK")
            .unwrap_or_else(|_| "libp2p,http,supabase".to_string());
        chain
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub async fn send_with_fallback(&self, peer_id: &str, payload: &[u8]) -> Result<()> {
        let mut last_err = None;
        for t in &self.transports {
            if !t.is_available().await {
                continue;
            }
            match t.send(peer_id, payload).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no mesh transports configured")))
    }

    pub fn active_transport_name(&self) -> Option<&'static str> {
        // best-effort: first available is active
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTransport {
        name: &'static str,
        available: bool,
        should_fail: bool,
    }

    #[async_trait::async_trait]
    impl FallbackTransport for MockTransport {
        async fn send(&self, _peer_id: &str, _payload: &[u8]) -> Result<()> {
            if self.should_fail {
                anyhow::bail!("mock fail {}", self.name)
            } else {
                Ok(())
            }
        }
        async fn is_available(&self) -> bool {
            self.available
        }
        fn name(&self) -> &'static str {
            self.name
        }
    }

    #[tokio::test]
    async fn test_fallback_skips_unavailable() {
        let t1 = std::sync::Arc::new(MockTransport {
            name: "libp2p",
            available: false,
            should_fail: false,
        });
        let t2 = std::sync::Arc::new(MockTransport {
            name: "http",
            available: true,
            should_fail: false,
        });
        let fb = FallbackMeshTransport::new(vec![t1, t2]);
        fb.send_with_fallback("peer1", b"hello").await.unwrap();
    }

    #[tokio::test]
    async fn test_fallback_tries_next_on_fail() {
        let t1 = std::sync::Arc::new(MockTransport {
            name: "libp2p",
            available: true,
            should_fail: true,
        });
        let t2 = std::sync::Arc::new(MockTransport {
            name: "http",
            available: true,
            should_fail: false,
        });
        let fb = FallbackMeshTransport::new(vec![t1, t2]);
        fb.send_with_fallback("peer1", b"hello").await.unwrap();
    }

    #[test]
    fn test_chain_parse() {
        let chain = FallbackMeshTransport::chain_from_env();
        assert!(!chain.is_empty());
    }
}
