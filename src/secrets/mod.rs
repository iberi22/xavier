//! Secrets management module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("Secret not found: {0}")]
    NotFound(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Approval denied for operation: {0}")]
    ApprovalDenied(String),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Database error: {0}")]
    DatabaseError(String),
}

pub type SecretResult<T> = Result<T, SecretError>;

#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    pub key: String,
    pub value: String,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret")
            .field("key", &"[REDACTED]")
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[deprecated(note = "SecretsManager is deprecated. Use Clavis or another secure provider.")]
#[derive(Default)]
#[allow(deprecated)]
pub struct SecretsManager;

#[allow(deprecated)]
impl SecretsManager {
    /// New.
    pub fn new() -> Self {
        Self
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        true
    }

    /// Store.
    pub fn store(&mut self, _key: String, _value: String) -> SecretResult<()> {
        Ok(())
    }

    /// Get.
    pub fn get(&self, key: &str) -> SecretResult<String> {
        Err(SecretError::NotFound(key.to_string()))
    }

    /// Delete.
    pub fn delete(&mut self, _key: &str) -> SecretResult<()> {
        Ok(())
    }

    /// Exists.
    pub fn exists(&self, _key: &str) -> bool {
        false
    }
}

// Lending engine
pub mod audit;
pub mod lending;
pub mod local;
pub mod local_vault;
pub mod openbao;
pub mod store;
#[cfg(test)]
mod tests;
pub mod vault;
