// SPDX-License-Identifier: MIT OR LICENSE-MESH
use crate::auth2::jwt::JwtManager;
use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
// use crate::cli::server::CliState;

pub struct RateLimiter {
    requests: RwLock<HashMap<String, Vec<Instant>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window_seconds: u64) -> Self {
        Self {
            requests: RwLock::new(HashMap::new()),
            max_requests,
            window: Duration::from_secs(window_seconds),
        }
    }

    pub async fn check(&self, ip: String) -> bool {
        let mut requests = self.requests.write().await;
        let now = Instant::now();
        let window_start = now - self.window;

        let ip_requests = requests.entry(ip).or_insert_with(Vec::new);
        ip_requests.retain(|&time| time > window_start);

        if ip_requests.len() >= self.max_requests {
            false
        } else {
            ip_requests.push(now);
            true
        }
    }
}

pub async fn auth_middleware(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    // 1. Rate Limiting (100 req/min)
    // In a real app we'd get the real IP, here we'll use a placeholder or header
    let client_ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Note: We need a global rate limiter. For now I'll use one in state or create a static one.
    // Given CliState doesn't have it yet, and I shouldn't modify it too much yet,
    // let's assume we'll add it to CliState or use a global lazy one.
    // For this implementation, I'll use a static one for simplicity in this step.
    static RATE_LIMITER: std::sync::LazyLock<RateLimiter> =
        std::sync::LazyLock::new(|| RateLimiter::new(100, 60));

    if !RATE_LIMITER.check(client_ip).await {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    // 2. JWT Validation
    let auth_header = req.headers().get(header::AUTHORIZATION);
    let token = auth_header
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_manager = JwtManager::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let claims = jwt_manager
        .validate_token(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Add claims to request extensions so handlers can use them
    req.extensions_mut().insert(claims);

    Ok(next.run(req).await)
}
