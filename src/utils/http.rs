// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! HTTP utility functions
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use std::sync::LazyLock;
use std::time::Duration;

pub static DEFAULT_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("xavier-internal/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build default HTTP client")
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_http_client_exists() {
        // Just verify it doesn't panic on access
        let _ = &*DEFAULT_HTTP_CLIENT;
    }
}
