use thiserror::Error;

use crate::config::ConfigError;

#[derive(Debug, Error)]
pub enum MarketError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("license {0} is not enabled by market.licenses")]
    LicenseNotEnabled(String),
    #[error("listing {0} not found")]
    ListingNotFound(String),
    #[error("listing {0} is revoked and cannot change or be bought")]
    ListingRevoked(String),
    #[error("identity request {0} not found")]
    IdentityRequestNotFound(String),
    #[error("request is not authenticated: {0}")]
    Unauthenticated(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("store schema version {0} is newer than this build supports")]
    SchemaTooNew(i64),
    #[error("stored record is corrupt: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}
