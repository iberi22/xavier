//! Middleware and setup for the CLI HTTP server.
//!
//! This module implements authentication and rate-limiting middleware used by the
//! CLI's HTTP API. It ensures secure access via token validation and prevents
//! resource exhaustion through global rate limits.

use crate::cli::config::resolve_http_token;
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tracing::warn;
use xavier::coordination::secrets::SecretLease;

/// Auth middleware.
pub async fn auth_middleware(
    State(state): State<CliState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if path == "/health"
        || path == "/headless/health"
        || path == "/v1/mesh/workspaces/query"
        || path == "/mesh/public/nodes"
        || path == "/v1/mesh/public/nodes"
        || path == "/node/founder/status"
        || path == "/v1/node/founder/status"
    {
        return next.run(req).await;
    }

    let expected_token = match resolve_http_token() {
        Ok(token) => token,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status":"error","message": format!("Token resolution failed: {e}")}),
            );
        }
    };

    let provided_token = req
        .headers()
        .get("X-Xavier-Token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            req.headers()
                .get("Authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        });

    let provided_token_str = provided_token.unwrap_or("");

    // 1. Check Root Token
    use subtle::ConstantTimeEq;
    let provided_bytes = provided_token_str.as_bytes();
    let expected_bytes = expected_token.as_bytes();
    let is_match: bool = provided_bytes.ct_eq(expected_bytes).into();

    if is_match {
        // Root token bypasses RBAC for now as "Super Admin"
        let mut req = req;
        req.extensions_mut().insert(SessionInfo {
            is_ephemeral: false,
            api_token: None,
            user_id: None,
            lease: None,
        });
        req.extensions_mut()
            .insert(xavier::security::auth::Claims::new(
                "root".to_string(),
                "admin@swal.dev".to_string(),
                xavier::security::auth::UserRole::Admin,
                chrono::Duration::hours(1),
            ));
        return next.run(req).await;
    }

    // 2. Check Lease Token (F3 - Proxy Authentication)
    if let Some(lease) = state.secrets_engine.get_lease(provided_token_str).await {
        if !lease.is_expired() {
            let mut req = req;
            req.extensions_mut().insert(SessionInfo {
                is_ephemeral: true,
                api_token: None,
                user_id: None,
                lease: Some(lease),
            });
            req.extensions_mut()
                .insert(xavier::security::auth::Claims::new(
                    "agent_lease".to_string(),
                    "agent@swal.dev".to_string(),
                    xavier::security::auth::UserRole::User,
                    chrono::Duration::hours(1),
                ));
            return next.run(req).await;
        }
    }

    // 3. Check Ephemeral Session (Zero-Trust Frontend)
    if state.session_manager.validate_session(provided_token_str) {
        let mut req = req;
        req.extensions_mut().insert(SessionInfo {
            is_ephemeral: true,
            api_token: None,
            user_id: None,
            lease: None,
        });
        req.extensions_mut()
            .insert(xavier::security::auth::Claims::new(
                "ephemeral_session".to_string(),
                "session@swal.dev".to_string(),
                xavier::security::auth::UserRole::User,
                chrono::Duration::hours(1),
            ));
        return next.run(req).await;
    }

    // 4. Check Persistent API Tokens
    if provided_token_str.starts_with("xav_") {
        let store = xavier::security::tokens::TokenStore::new();
        if let Ok(Some(token_meta)) = store.validate_token(provided_token_str).await {
            // Scope validation
            let has_scope = match path {
                p if p.starts_with("/memory/search") || p.starts_with("/v1/memories/search") => {
                    token_meta.scopes.contains(&"read".to_string())
                        || token_meta.scopes.contains(&"all".to_string())
                }
                p if p.starts_with("/memory/add") || p.starts_with("/v1/memories") => {
                    token_meta.scopes.contains(&"write".to_string())
                        || token_meta.scopes.contains(&"all".to_string())
                }
                _ => true, // Default allow for other endpoints for now, or refine as needed
            };

            if !has_scope {
                return json_response(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({"status":"error","message":"Insufficient scopes"}),
                );
            }

            // Integrate RBAC authorization check for persistent tokens
            let permission = match path {
                p if p.contains("/add") || p.contains("/update") || p.contains("/delete") => {
                    xavier::enterprise::rbac::Permission::Write
                }
                _ => xavier::enterprise::rbac::Permission::Read,
            };

            // Scaffolding for RBAC authorize call
            if let Err(e) = xavier::enterprise::rbac::authorize(
                uuid::Uuid::nil(), // Placeholder for real user_id from token_meta
                permission,
                path.to_string(),
            ) {
                return json_response(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({"status":"error","message": e.to_string()}),
                );
            }

            let mut req = req;
            req.extensions_mut().insert(SessionInfo {
                is_ephemeral: false,
                api_token: Some(token_meta),
                user_id: None,
                lease: None,
            });
            req.extensions_mut()
                .insert(xavier::security::auth::Claims::new(
                    "api_token".to_string(),
                    "api_token@swal.dev".to_string(),
                    xavier::security::auth::UserRole::User,
                    chrono::Duration::hours(1),
                ));
            return next.run(req).await;
        }
    }

    // 5. Check JWT Tokens
    if let Ok(secret) = std::env::var("XAVIER_JWT_SECRET") {
        if let Ok(claims) =
            xavier::security::auth::validate_jwt(provided_token_str, secret.as_bytes())
        {
            let mut req = req;
            req.extensions_mut().insert(SessionInfo {
                is_ephemeral: false,
                api_token: None,
                user_id: Some(claims.sub.clone()),
                lease: None,
            });
            req.extensions_mut().insert(claims);
            return next.run(req).await;
        }
    }

    json_response(
        StatusCode::UNAUTHORIZED,
        serde_json::json!({"status":"error","message":"Unauthorized"}),
    )
}

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub is_ephemeral: bool,
    pub api_token: Option<xavier::security::tokens::ApiTokenMetadata>,
    pub user_id: Option<String>,
    pub lease: Option<SecretLease>,
}

/// Rate limit middleware.
/// Limitador de `/auth/*` EN MEMORIA (ventana fija de 60 s).
///
/// No consulta la base a proposito. El conteo anterior vivia en la tabla `rate_limit_usage` y en un
/// nodo cargado la escritura del conteo fallaba en silencio: medido en vivo, 25 intentos de login
/// seguidos no dejaron NI UNA fila nueva y jamas aparecio un 429. La puerta de la fuerza bruta no
/// puede depender de una escritura que puede fallar.
static AUTH_LIMITER: LazyLock<Mutex<HashMap<String, (Instant, u32)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Nucleo de ventana fija. Separado para poder probarlo sin esperar 60 s reales.
fn auth_window_allow(
    ventanas: &mut HashMap<String, (Instant, u32)>,
    clave: &str,
    limite: u32,
    ahora: Instant,
    ventana: Duration,
) -> bool {
    match ventanas.get_mut(clave) {
        Some((inicio, cuenta)) => {
            if ahora.duration_since(*inicio) >= ventana {
                *inicio = ahora;
                *cuenta = 1;
                1 <= limite
            } else {
                *cuenta += 1;
                *cuenta <= limite
            }
        }
        // Primera peticion de esta clave: tambien respeta el limite (con limite 0 se corta).
        None => {
            ventanas.insert(clave.to_string(), (ahora, 1));
            1 <= limite
        }
    }
}

/// Registra un intento de `/auth/*` y dice si se permite (ventana real de 60 s).
fn auth_rate_allow(provider: &str, limite: u32) -> bool {
    let mut m = AUTH_LIMITER.lock().unwrap_or_else(|e| e.into_inner());
    auth_window_allow(
        &mut m,
        provider,
        limite,
        Instant::now(),
        Duration::from_secs(60),
    )
}


/// Limitador de tasa EXCLUSIVO del nido de autenticacion.
///
/// Se monta con `from_fn` sobre el router de `auth_routes`, es decir DENTRO del nest `/auth`, y por
/// eso no mira el path: axum recorta el prefijo al enrutar hacia el router anidado, de modo que
/// dentro de esta capa la ruta es `/login` y no `/auth/login`. Comprobado a golpes: la condicion
/// `path.starts_with("/auth/")` jamas era cierta aqui y el limite no disparaba nunca.
///
/// Como todo lo que entra en este nest es autenticacion, se limita todo por igual. El contador es en
/// memoria (ver AUTH_LIMITER) y el tope por defecto es 20 por minuto (XAVIER_AUTH_RATE_LIMIT).
pub async fn auth_rate_limit_middleware(req: Request<Body>, next: Next) -> Response {
    let mut provider = if let Some(addr) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        format!("ip:{}", addr.0.ip())
    } else {
        "api_gateway".to_string()
    };
    if let Some(session) = req.extensions().get::<SessionInfo>() {
        if let Some(lease) = &session.lease {
            provider = format!("agent:{}", lease.agent_id);
        }
    }

    let limite = std::env::var("XAVIER_AUTH_RATE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    if !auth_rate_allow(&provider, limite) {
        warn!("Auth rate limit exceeded for {}", provider);
        return json_response(
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({
                "status": "error",
                "message": format!(
                    "Demasiados intentos de autenticacion. Maximo {} por minuto.",
                    limite
                ),
            }),
        );
    }

    next.run(req).await
}

/// Limitador de tasa que SI se ejecuta en este servidor.
///
/// Lo trae `server.rs` por el import glob `pub use crate::cli::http_setup::*` y lo monta con
/// `from_fn_with_state` sobre los grupos de rutas, incluido el nest de `/auth`.
///
/// OJO: `crate::middleware::token_bucket::rate_limit_middleware` es OTRA funcion con la misma
/// intencion, y la usan las rutas del crate lib (`src/adapters/inbound/http/routes.rs:272`).
/// Parchear la que no corresponde no cambia nada de lo que llega por HTTP a este servidor: eso ya
/// paso dos veces, asi que si tocas limites, comprueba PRIMERO cual esta montado donde.
pub async fn rate_limit_middleware(
    State(state): State<CliState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Default provider based on IP if available
    let mut provider = if let Some(addr) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        format!("ip:{}", addr.0.ip())
    } else {
        "api_gateway".to_string()
    };

    // Override with agent_id if lease is present
    if let Some(session) = req.extensions().get::<SessionInfo>() {
        if let Some(lease) = &session.lease {
            provider = format!("agent:{}", lease.agent_id);
        }
    }

    // F3: Proxy RPM Rate Limiting
    if path.starts_with("/v1/proxy/") {
        let limit = std::env::var("XAVIER_PROXY_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);

        match state.rate_manager.check_rpm_limit(&provider, limit).await {
            Ok(allowed) => {
                if !allowed {
                    warn!("Proxy rate limit exceeded for {}", provider);
                    return json_response(
                        StatusCode::TOO_MANY_REQUESTS,
                        serde_json::json!({
                            "status": "error",
                            "message": "Proxy rate limit exceeded. Max 60 requests per minute.",
                        }),
                    );
                }
            }
            Err(e) => {
                warn!("Failed to check proxy rate limit: {}", e);
            }
        }

        // Record the request immediately for accurate RPM tracking
        if let Err(e) = state
            .rate_manager
            .track_request(&provider, 1, 200, 0.0, false)
            .await
        {
            warn!("Failed to track proxy request: {}", e);
        }
    }

    match state.rate_manager.get_status(&provider).await {
        Ok(status) => {
            if let Some(until) = status.rate_limited_until {
                if until > chrono::Utc::now() {
                    warn!("Global rate limit reached, blocking request");
                    return json_response(
                        StatusCode::TOO_MANY_REQUESTS,
                        serde_json::json!({
                            "status": "error",
                            "message": "Rate limit exceeded. Please try again later.",
                            "retry_after": until.to_rfc3339()
                        }),
                    );
                }
            }
        }
        Err(e) => {
            warn!("Failed to check rate limit: {}", e);
        }
    }

    let response = next.run(req).await;

    if let Err(e) = state
        .rate_manager
        .track_request(&provider, 1, response.status().as_u16(), 0.0, false)
        .await
    {
        warn!("Failed to track rate limit usage: {}", e);
    }

    response
}

#[cfg(test)]
mod auth_limit_tests {
    use super::auth_window_allow;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    #[test]
    fn corta_al_llegar_al_limite_y_reabre_en_la_ventana_siguiente() {
        let mut v = HashMap::new();
        let t0 = Instant::now();
        let ventana = Duration::from_secs(60);
        assert!(auth_window_allow(&mut v, "k", 3, t0, ventana));
        assert!(auth_window_allow(&mut v, "k", 3, t0, ventana));
        assert!(auth_window_allow(&mut v, "k", 3, t0, ventana));
        assert!(!auth_window_allow(&mut v, "k", 3, t0, ventana));
        assert!(!auth_window_allow(
            &mut v,
            "k",
            3,
            t0 + Duration::from_secs(59),
            ventana
        ));
        assert!(auth_window_allow(
            &mut v,
            "k",
            3,
            t0 + Duration::from_secs(61),
            ventana
        ));
    }

    #[test]
    fn cada_clave_cuenta_aparte() {
        let mut v = HashMap::new();
        let t0 = Instant::now();
        let ventana = Duration::from_secs(60);
        assert!(auth_window_allow(&mut v, "a", 1, t0, ventana));
        assert!(!auth_window_allow(&mut v, "a", 1, t0, ventana));
        assert!(auth_window_allow(&mut v, "b", 1, t0, ventana));
    }

    #[test]
    fn limite_cero_corta_siempre() {
        let mut v = HashMap::new();
        let t0 = Instant::now();
        assert!(!auth_window_allow(
            &mut v,
            "k",
            0,
            t0,
            Duration::from_secs(60)
        ));
    }
}
