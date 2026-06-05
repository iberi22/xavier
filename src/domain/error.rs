use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}
