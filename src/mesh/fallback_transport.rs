//! FallbackMeshTransport — libp2p→http→supabase realtime (WAVE-1.03 / Ola 23.04)
//!
//! Tries mesh transports in order with bounded per-backend retry and exponential backoff
//! until one succeeds, returning a degraded signal when fallback or retries occur.

use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;

/// Bounded retry configuration
pub const MAX_ATTEMPTS_PER_BACKEND: usize = 2;
pub const INITIAL_BACKOFF_MS: u64 = 50;
pub const MAX_BACKOFF_MS: u64 = 200;

/// Result outcome describing which backend served the send request and whether execution was degraded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOutcome {
    pub backend: &'static str,
    pub degraded: bool,
}

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

    pub async fn send_with_fallback(&self, peer_id: &str, payload: &[u8]) -> Result<SendOutcome> {
        let mut last_err = None;
        for (idx, t) in self.transports.iter().enumerate() {
            if !t.is_available().await {
                continue;
            }
            let mut backoff_ms = INITIAL_BACKOFF_MS;
            for attempt in 1..=MAX_ATTEMPTS_PER_BACKEND {
                if attempt > 1 {
                    sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
                }
                match t.send(peer_id, payload).await {
                    Ok(()) => {
                        let degraded = idx > 0 || attempt > 1;
                        return Ok(SendOutcome {
                            backend: t.name(),
                            degraded,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            backend = t.name(),
                            attempt = attempt,
                            max_attempts = MAX_ATTEMPTS_PER_BACKEND,
                            error = %e,
                            "Fallback transport send attempt failed"
                        );
                        last_err = Some(e);
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no mesh transports configured")))
    }

    pub fn active_transport_name(&self) -> Option<&'static str> {
        // best-effort: returns primary transport name if available
        self.transports.first().map(|t| t.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockTransport {
        name: &'static str,
        available: bool,
        fails_remaining: AtomicUsize,
        attempts: AtomicUsize,
    }

    impl MockTransport {
        fn new(name: &'static str, available: bool, fails_count: usize) -> Self {
            Self {
                name,
                available,
                fails_remaining: AtomicUsize::new(fails_count),
                attempts: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl FallbackTransport for MockTransport {
        async fn send(&self, _peer_id: &str, _payload: &[u8]) -> Result<()> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            loop {
                let current = self.fails_remaining.load(Ordering::SeqCst);
                if current == 0 {
                    return Ok(());
                }
                if self
                    .fails_remaining
                    .compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    anyhow::bail!("mock fail {}", self.name);
                }
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
        let t1 = std::sync::Arc::new(MockTransport::new("libp2p", false, 0));
        let t2 = std::sync::Arc::new(MockTransport::new("http", true, 0));
        let fb = FallbackMeshTransport::new(vec![t1.clone(), t2.clone()]);
        let outcome = fb.send_with_fallback("peer1", b"hello").await.unwrap();
        assert_eq!(
            outcome,
            SendOutcome {
                backend: "http",
                degraded: true,
            }
        );
        assert_eq!(t1.attempts.load(Ordering::SeqCst), 0);
        assert_eq!(t2.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_fallback_tries_next_on_fail() {
        let t1 = std::sync::Arc::new(MockTransport::new("libp2p", true, 999));
        let t2 = std::sync::Arc::new(MockTransport::new("http", true, 0));
        let fb = FallbackMeshTransport::new(vec![t1.clone(), t2.clone()]);
        let outcome = fb.send_with_fallback("peer1", b"hello").await.unwrap();
        assert_eq!(
            outcome,
            SendOutcome {
                backend: "http",
                degraded: true,
            }
        );
        assert_eq!(t1.attempts.load(Ordering::SeqCst), MAX_ATTEMPTS_PER_BACKEND);
        assert_eq!(t2.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_fallback_primary_success_not_degraded() {
        let t1 = std::sync::Arc::new(MockTransport::new("libp2p", true, 0));
        let t2 = std::sync::Arc::new(MockTransport::new("http", true, 0));
        let fb = FallbackMeshTransport::new(vec![t1.clone(), t2.clone()]);
        let outcome = fb.send_with_fallback("peer1", b"hello").await.unwrap();
        assert_eq!(
            outcome,
            SendOutcome {
                backend: "libp2p",
                degraded: false,
            }
        );
        assert_eq!(t1.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(t2.attempts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_fallback_retry_success_degraded() {
        let t1 = std::sync::Arc::new(MockTransport::new("libp2p", true, 1));
        let fb = FallbackMeshTransport::new(vec![t1.clone()]);
        let outcome = fb.send_with_fallback("peer1", b"hello").await.unwrap();
        assert_eq!(
            outcome,
            SendOutcome {
                backend: "libp2p",
                degraded: true,
            }
        );
        assert_eq!(t1.attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_fallback_bounded_retry_all_fail() {
        let t1 = std::sync::Arc::new(MockTransport::new("libp2p", true, 999));
        let t2 = std::sync::Arc::new(MockTransport::new("http", true, 999));
        let fb = FallbackMeshTransport::new(vec![t1.clone(), t2.clone()]);
        let res = fb.send_with_fallback("peer1", b"hello").await;
        assert!(res.is_err());
        assert_eq!(t1.attempts.load(Ordering::SeqCst), MAX_ATTEMPTS_PER_BACKEND);
        assert_eq!(t2.attempts.load(Ordering::SeqCst), MAX_ATTEMPTS_PER_BACKEND);
    }

    #[test]
    fn test_chain_parse() {
        let chain = FallbackMeshTransport::chain_from_env();
        assert!(!chain.is_empty());
    }
}
