use parking_lot::Mutex;
use std::time::{Duration, Instant};

/// A simple Token Bucket rate limiter.
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    fill_rate: f64, // tokens per second
    last_fill: Instant,
}

impl TokenBucket {
    /// Create a new Token Bucket.
    /// capacity: max tokens in bucket.
    /// fill_rate: tokens added per second.
    pub fn new(capacity: f64, fill_rate: f64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            fill_rate,
            last_fill: Instant::now(),
        }
    }

    /// Try to consume tokens from the bucket.
    /// Returns true if successful, false if not enough tokens.
    pub fn try_consume(&mut self, amount: f64) -> bool {
        self.refill();
        if self.tokens >= amount {
            self.tokens -= amount;
            true
        } else {
            false
        }
    }

    /// Returns the number of tokens currently in the bucket.
    pub fn tokens(&mut self) -> f64 {
        self.refill();
        self.tokens
    }

    /// Returns the time until at least `amount` tokens will be available.
    pub fn retry_after(&mut self, amount: f64) -> Duration {
        self.refill();
        if self.tokens >= amount {
            Duration::from_secs(0)
        } else {
            let needed = amount - self.tokens;
            Duration::from_secs_f64(needed / self.fill_rate)
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_fill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.fill_rate).min(self.capacity);
        self.last_fill = now;
    }
}

/// Thread-safe wrapper for TokenBucket.
pub struct RateLimiter {
    bucket: Mutex<TokenBucket>,
}

impl RateLimiter {
    /// New.
    pub fn new(capacity: f64, fill_rate: f64) -> Self {
        Self {
            bucket: Mutex::new(TokenBucket::new(capacity, fill_rate)),
        }
    }

    /// Try consume sync.
    pub fn try_consume_sync(&self, amount: f64) -> bool {
        let mut bucket = self.bucket.lock();
        bucket.try_consume(amount)
    }

    /// Tokens sync.
    pub fn tokens_sync(&self) -> f64 {
        let mut bucket = self.bucket.lock();
        bucket.tokens()
    }

    /// Retry after sync.
    pub fn retry_after_sync(&self, amount: f64) -> Duration {
        let mut bucket = self.bucket.lock();
        bucket.retry_after(amount)
    }

    /// Try consume.
    pub async fn try_consume(&self, amount: f64) -> bool {
        self.try_consume_sync(amount)
    }

    /// Tokens.
    pub async fn tokens(&self) -> f64 {
        self.tokens_sync()
    }

    /// Retry after.
    pub async fn retry_after(&self, amount: f64) -> Duration {
        self.retry_after_sync(amount)
    }
}

/// Thread-safe per-IP rate limiter using token bucket strategy.
pub struct IpRateLimiter {
    capacity: f64,
    fill_rate: f64,
    buckets: Mutex<std::collections::HashMap<String, TokenBucket>>,
}

impl IpRateLimiter {
    /// Create a new IpRateLimiter.
    /// capacity: max tokens per bucket.
    /// fill_rate: token refill rate per second.
    pub fn new(capacity: f64, fill_rate: f64) -> Self {
        Self {
            capacity,
            fill_rate,
            buckets: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Try to consume `amount` tokens for a specific IP.
    /// Returns `(true, Duration::ZERO)` if allowed, or `(false, retry_after)` if limited.
    pub fn try_consume(&self, ip: &str, amount: f64) -> (bool, Duration) {
        let mut buckets = self.buckets.lock();
        let bucket = buckets
            .entry(ip.to_string())
            .or_insert_with(|| TokenBucket::new(self.capacity, self.fill_rate));
        if bucket.try_consume(amount) {
            (true, Duration::ZERO)
        } else {
            let retry_after = bucket.retry_after(amount);
            (false, retry_after)
        }
    }
}

static GLOBAL_IP_RATE_LIMITER: std::sync::LazyLock<IpRateLimiter> =
    std::sync::LazyLock::new(|| IpRateLimiter::new(100.0, 1.0));

/// Cubo de tasa ESPECIFICO de `/auth/*`, mas estricto que el general.
///
/// El general tiene capacidad 100 y recarga 1/s: medido en vivo, 60 intentos de login seguidos no
/// llegaban a agotarlo y no aparecia ni un 429, con lo que la puerta de la fuerza bruta quedaba
/// abierta. Aqui se usa XAVIER_AUTH_RATE_LIMIT (defecto 20 por minuto): capacidad = ese numero y
/// recarga = numero/60 por segundo, es decir el mismo tope repartido en la ventana de un minuto.
static AUTH_IP_RATE_LIMITER: std::sync::LazyLock<IpRateLimiter> = std::sync::LazyLock::new(|| {
    let rpm = std::env::var("XAVIER_AUTH_RATE_LIMIT")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v: &f64| *v > 0.0)
        .unwrap_or(20.0);
    IpRateLimiter::new(rpm, rpm / 60.0)
});

/// Axum rate-limiting middleware mounted by default on API routes.
///
/// Throttles requests per IP (100 capacity, 60 req/min refill rate).
/// Returns HTTP status 429 Too Many Requests with Retry-After header when exceeded.
pub async fn rate_limit_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{header, HeaderValue, StatusCode};
    use axum::response::IntoResponse;
    use axum::Json;

    let client_ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "127.0.0.1".to_string());

    // /auth/* va contra el cubo estricto; el resto contra el general.
    let limiter = if req.uri().path().starts_with("/auth/") {
        &AUTH_IP_RATE_LIMITER
    } else {
        &GLOBAL_IP_RATE_LIMITER
    };
    let (allowed, retry_after) = limiter.try_consume(&client_ip, 1.0);
    if !allowed {
        let retry_secs = retry_after.as_secs().max(1);
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "status": "error",
                "message": "Too Many Requests"
            })),
        )
            .into_response();

        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&retry_secs.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("1")),
        );
        return response;
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_rate_limiter_try_consume() {
        let limiter = RateLimiter::new(2.0, 1.0);
        assert!(limiter.try_consume(1.0).await);
        assert!(limiter.try_consume(1.0).await);
        assert!(!limiter.try_consume(1.0).await);
        assert!(limiter.retry_after(1.0).await > Duration::ZERO);
    }

    #[tokio::test]
    async fn test_rate_limit_middleware_429_retry_after() {
        let app = Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(rate_limit_middleware));

        let unique_ip = format!("10.99.88.{}", ulid::Ulid::new().to_string());

        let mut hit_429 = false;
        let mut retry_after_header = None;

        for _ in 0..105 {
            let req = Request::builder()
                .uri("/test")
                .header("x-forwarded-for", &unique_ip)
                .body(Body::empty())
                .unwrap();

            let response = app.clone().oneshot(req).await.unwrap();
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                hit_429 = true;
                if let Some(h) = response.headers().get("retry-after") {
                    retry_after_header = Some(h.to_str().unwrap().to_string());
                }
                break;
            }
        }

        assert!(hit_429, "Expected 429 status after 100 requests");
        assert!(retry_after_header.is_some(), "Expected Retry-After header");
    }

    /// El cubo de `/auth/*` debe cortar antes que el general.
    ///
    /// El general (capacidad 100) no corta 25 peticiones seguidas; el de /auth si, porque su tope
    /// por defecto es 20 por minuto. Es la diferencia entre tener puerta y no tenerla.
    #[tokio::test]
    async fn test_auth_tiene_cubo_mas_estricto_que_el_general() {
        use axum::http::StatusCode;

        let app = Router::new()
            .route("/auth/login", get(|| async { "ok" }))
            .route("/v1/otra", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(rate_limit_middleware));

        let ip = format!("10.77.66.{}", ulid::Ulid::new());
        let mut codigos = Vec::new();
        for _ in 0..25 {
            let req = Request::builder()
                .uri("/auth/login")
                .header("x-forwarded-for", &ip)
                .body(Body::empty())
                .unwrap();
            codigos.push(app.clone().oneshot(req).await.unwrap().status());
        }
        assert!(
            codigos.contains(&StatusCode::TOO_MANY_REQUESTS),
            "el cubo de /auth no corto en 25 intentos: {codigos:?}"
        );

        // El mismo numero de peticiones a una ruta normal NO se corta: el general permite 100.
        let ip2 = format!("10.77.55.{}", ulid::Ulid::new());
        let mut normales = Vec::new();
        for _ in 0..25 {
            let req = Request::builder()
                .uri("/v1/otra")
                .header("x-forwarded-for", &ip2)
                .body(Body::empty())
                .unwrap();
            normales.push(app.clone().oneshot(req).await.unwrap().status());
        }
        assert!(
            normales.iter().all(|c| *c == StatusCode::OK),
            "el cubo general no deberia cortar 25 peticiones: {normales:?}"
        );
    }
}
