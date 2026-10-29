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
        Err(e) => {
            let detail = e.to_string().to_lowercase();
            if e.is_connect()
                || e.is_timeout()
                || e.is_request()
                || detail.contains("connection refused")
                || detail.contains("connect")
                || detail.contains("timed out")
                || detail.contains("timeout")
            {
                4
            } else {
                1
            }
        }
    }
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
}
