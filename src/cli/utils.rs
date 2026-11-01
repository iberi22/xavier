//! CLI utility functions

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Estimate tokens.
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

pub struct ProxyErrorWrapper(pub xavier::domain::proxy::ProxyError);

impl IntoResponse for ProxyErrorWrapper {
    fn into_response(self) -> Response {
        let (status, message) = match self.0 {
            xavier::domain::proxy::ProxyError::RateLimited => {
                (StatusCode::TOO_MANY_REQUESTS, self.0.to_string())
            }
            xavier::domain::proxy::ProxyError::ProviderError(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e)
            }
            xavier::domain::proxy::ProxyError::Internal(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e)
            }
            xavier::domain::proxy::ProxyError::SecretError(e) => (StatusCode::FORBIDDEN, e),
            xavier::domain::proxy::ProxyError::InvalidRequest(e) => (StatusCode::BAD_REQUEST, e),
        };
        (status, message).into_response()
    }
}

/// Exit code for a command that exists but is not supported on this version
/// (distinct from `1`, a failed operation). Documented in docs/reference/CLI.md.
pub const EXIT_UNSUPPORTED: i32 = 5;

/// Calculate exit code based on HTTP result.
pub fn exit_code_for(result: &Result<reqwest::Response, reqwest::Error>) -> i32 {
    match result {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                0
            } else if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                3
            } else {
                1
            }
        }
        // Only transport failures (could not connect / timed out) map to 4.
        // Other request errors (builder, redirect, decode, ...) are generic.
        Err(e) => {
            if e.is_connect() || e.is_timeout() {
                4
            } else {
                1
            }
        }
    }
}

pub fn resolve_active_workspace() -> String {
    if let Ok(val) = std::env::var("XAVIER_WORKSPACE_ID") {
        if !val.is_empty() && val != "default" {
            return val;
        }
    }
    let data_dir = crate::settings::XavierSettings::resolve_data_dir();
    let path = data_dir.join("active_space");
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let id = content.trim();
            if !id.is_empty() {
                return id.to_string();
            }
        }
    }
    let state_dir = std::env::var("XAVIER_STATE_DIR").unwrap_or_else(|_| ".".to_string());
    let active_space_file = std::path::PathBuf::from(&state_dir)
        .join(".xavier")
        .join("active_space");
    if active_space_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&active_space_file) {
            let id = content.trim();
            if !id.is_empty() {
                return id.to_string();
            }
        }
    }
    if let Ok(val) = std::env::var("XAVIER_WORKSPACE_ID") {
        if !val.is_empty() {
            return val;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_DEFAULT_WORKSPACE_ID") {
        if !val.is_empty() {
            return val;
        }
    }
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    // Helper to create a dummy response with a specific status code
    fn mock_response(status: StatusCode) -> Result<reqwest::Response, reqwest::Error> {
        let resp = axum::http::Response::builder()
            .status(status)
            .body("")
            .unwrap();
        Ok(resp.into())
    }

    #[test]
    fn test_exit_code_for() {
        assert_eq!(exit_code_for(&mock_response(StatusCode::OK)), 0);
        assert_eq!(exit_code_for(&mock_response(StatusCode::UNAUTHORIZED)), 3);
        assert_eq!(exit_code_for(&mock_response(StatusCode::FORBIDDEN)), 3);
        assert_eq!(
            exit_code_for(&mock_response(StatusCode::INTERNAL_SERVER_ERROR)),
            1
        );

        // Simulating reqwest errors without making actual requests is tricky,
        // but we can create a timeout error by actually hitting a blackhole or
        // we can just test the Ok branch. Since creating reqwest errors manually is private,
        // we will rely on the Ok tests here and let e2e tests cover the Err branch.
    }

    #[tokio::test]
    async fn non_connect_request_error_is_generic_failure() {
        // A URL whose host contains "connect" and "timeout" must not be
        // classified by substring; an invalid scheme is a builder error.
        let result = reqwest::Client::new()
            .get("unsupported://connect-timeout.invalid/")
            .send()
            .await;
        let err = result.as_ref().expect_err("unsupported scheme must fail");
        assert!(!err.is_connect() && !err.is_timeout());
        assert_eq!(exit_code_for(&result), 1);
    }
}
