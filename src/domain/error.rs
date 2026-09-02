//! Domain error types
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
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

impl AppError {
    /// Returns the category name for this error variant.
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Internal(_) => "internal",
            Self::ConfigError(_) => "config_error",
            Self::InvalidInput(_) => "invalid_input",
        }
    }

    /// Returns `true` if the error is an [`AppError::Internal`] variant.
    pub const fn is_internal(&self) -> bool {
        matches!(self, Self::Internal(_))
    }

    /// Returns `true` if the error is an [`AppError::ConfigError`] variant.
    pub const fn is_config_error(&self) -> bool {
        matches!(self, Self::ConfigError(_))
    }

    /// Returns `true` if the error is an [`AppError::InvalidInput`] variant.
    pub const fn is_invalid_input(&self) -> bool {
        matches!(self, Self::InvalidInput(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_error_const_helpers() {
        let err1 = AppError::Internal("test internal".to_string());
        assert_eq!(err1.category(), "internal");
        assert!(err1.is_internal());
        assert!(!err1.is_config_error());
        assert!(!err1.is_invalid_input());

        let err2 = AppError::ConfigError("test config".to_string());
        assert_eq!(err2.category(), "config_error");
        assert!(!err2.is_internal());
        assert!(err2.is_config_error());
        assert!(!err2.is_invalid_input());

        let err3 = AppError::InvalidInput("test input".to_string());
        assert_eq!(err3.category(), "invalid_input");
        assert!(!err3.is_internal());
        assert!(!err3.is_config_error());
        assert!(err3.is_invalid_input());
    }
}
