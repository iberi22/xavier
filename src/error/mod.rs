//! Standardized API Error Handling
//!
//! Provides unified error response format, consistent error codes, and structured logging.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Uniform error response payload sent to the client.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ErrorResponse {
    /// Always "error"
    pub status: String,
    /// Consistent error code (e.g., "NOT_FOUND")
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Backward-compatible field matching 'message'
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Optional additional structured details about the error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    /// Unix timestamp when the error occurred
    pub timestamp: i64,
}

/// Consolidated API Error types mapped to HTTP status codes.
#[derive(Debug, Clone)]
pub enum ApiError {
    Internal(String),
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    Conflict(String),
    Validation(String),
    Security(String),
    NotImplemented(String),
}

impl ApiError {
    /// Internal.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Not found.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Bad request.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    /// Unauthorized.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    /// Forbidden.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    /// Conflict.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    /// Validation.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// Security.
    pub fn security(message: impl Into<String>) -> Self {
        Self::Security(message.into())
    }

    /// Creates a new ApiError with a custom status code, code string, and message.
    pub fn new(status_code: StatusCode, code: &str, message: impl Into<String>) -> Self {
        match code {
            _ if status_code == StatusCode::NOT_IMPLEMENTED => Self::NotImplemented(message.into()),
            "INTERNAL_ERROR" => Self::Internal(message.into()),
            "NOT_FOUND" => Self::NotFound(message.into()),
            "BAD_REQUEST" => Self::BadRequest(message.into()),
            "UNAUTHORIZED" => Self::Unauthorized(message.into()),
            "FORBIDDEN" => Self::Forbidden(message.into()),
            "CONFLICT" => Self::Conflict(message.into()),
            "VALIDATION_ERROR" => Self::Validation(message.into()),
            "SECURITY_VIOLATION" => Self::Security(message.into()),
            "NOT_IMPLEMENTED" => Self::NotImplemented(message.into()),
            _ => Self::Internal(message.into()), // Default fallback
        }
    }

    /// Retrieve the standard code string and status code.
    pub fn details(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                msg.clone(),
            ),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, "NOT_FOUND", msg.clone()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", msg.clone()),
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", msg.clone()),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN", msg.clone()),
            Self::Conflict(msg) => (StatusCode::CONFLICT, "CONFLICT", msg.clone()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::Security(msg) => (StatusCode::FORBIDDEN, "SECURITY_VIOLATION", msg.clone()),
            Self::NotImplemented(msg) => {
                (StatusCode::NOT_IMPLEMENTED, "NOT_IMPLEMENTED", msg.clone())
            }
        }
    }

    /// Helper to convert this ApiError into a response with 200 OK status code,
    /// which some legacy or backward-compatible endpoints require, while keeping
    /// the standardized JSON error body.
    pub fn into_ok_response(self) -> Response {
        let (_, code_str, message) = self.details();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Structured logging
        tracing::error!(
            status_code = 200,
            error_code = %code_str,
            error_message = %message,
            "api_error"
        );

        let payload = ErrorResponse {
            status: "error".to_string(),
            code: code_str.to_string(),
            message: message.clone(),
            error: Some(message),
            details: None,
            timestamp,
        };

        (StatusCode::OK, Json(payload)).into_response()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status_code, code_str, message) = self.details();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Structured logging of the error
        tracing::error!(
            status_code = %status_code.as_u16(),
            error_code = %code_str,
            error_message = %message,
            "api_error"
        );

        let payload = ErrorResponse {
            status: "error".to_string(),
            code: code_str.to_string(),
            message: message.clone(),
            error: Some(message),
            details: None,
            timestamp,
        };

        (status_code, Json(payload)).into_response()
    }
}
