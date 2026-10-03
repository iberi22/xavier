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
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tracing::warn;
use xavier::coordination::secrets::SecretLease;
use xavier::espacio::{can, SpaceAction, SpaceManager, SpaceRole};

/// What a persistent `xav_` token must hold to reach a route.
///
/// The previous policy was a match with a `_ => true` arm, so every route
/// added after it inherited "allowed". An enum of requirements cannot be
/// extended by accident: a new route falls to `Read` or `Write` by method,
/// and the routes that must stay unreachable by scopes are named.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RequiredScope {
    Read,
    Write,
    /// No scope unlocks these — not even `all`. Root token only.
    RootOnly,
}

/// Second, independent layer behind the route-level role gate: prefixes a
/// scoped token can never reach, whatever its scopes say.
const ROOT_ONLY_PREFIXES: &[&str] = &[
    "/secrets/",
    "/security/tokens",
    "/v1/security/",
    "/v1/proxy/request",
    // Clavis reads and writes the node's whole secret vault (JWT_PRIVATE_KEY,
    // DB_MASTER_KEY, node_secret_*). It used to stop at the role gate alone,
    // so a scoped `xav_` token holding `all` walked past the scope layer.
    "/v1/clavis",
];

/// POST routes that only read, so they need `read` and not `write`.
const READ_ONLY_POST_PREFIXES: &[&str] = &["/memory/search", "/v1/memories/search"];

pub(crate) fn required_scope(method: &Method, path: &str) -> RequiredScope {
    if ROOT_ONLY_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return RequiredScope::RootOnly;
    }
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
        || READ_ONLY_POST_PREFIXES.iter().any(|p| path.starts_with(p))
    {
        RequiredScope::Read
    } else {
        RequiredScope::Write
    }
}

pub(crate) fn token_satisfies(scopes: &[String], required: &RequiredScope) -> bool {
    let has = |scope: &str| scopes.iter().any(|s| s == scope);
    match required {
        RequiredScope::RootOnly => false,
        RequiredScope::Read => has("read") || has("all"),
        RequiredScope::Write => has("write") || has("all"),
    }
}

/// A lease token is a capability for the proxy, not a general session.
///
/// It is handed to a subprocess for one outbound call; it must not become a
/// node-wide session, so it is refused everywhere except the proxy.
pub(crate) fn lease_may_access(path: &str) -> bool {
    path.starts_with("/v1/proxy/")
}

/// Path allowlist for `xsp_` space tokens, plus the role check.
///
/// Deny by default. Today a space token reaches exactly the routes that are
/// namespaced by space id in the router, and only for ITS OWN space:
/// `GET|HEAD /api/v1/espacio/spaces/{own}` (Read) and
/// `DELETE /api/v1/espacio/spaces/{own}` (Admin). Anything else, including
/// any other sub-path, trailing slash or other space, is refused.
///
/// TODO(WP-13m): `/memory*`, `/v1/memories*` and `/mcp*` are deliberately NOT
/// allowed. Their handlers use the workspace-global `CliState`, not the
/// token's space, so allowing them would give a space token the whole node's
/// memory. Add them back only once those handlers are namespaced per space.
///
/// Paths with encoded or dot segments are refused outright so no
/// normalisation step can turn an allowed path into another route.
pub(crate) fn space_token_may_access(
    path: &str,
    own_space_id: &str,
    role: SpaceRole,
    method: &Method,
) -> bool {
    if !path.starts_with('/')
        || path.contains('%')
        || path.contains('\\')
        || path.contains("//")
        || path.split('/').any(|seg| seg == "." || seg == "..")
    {
        return false;
    }
    if required_scope(method, path) == RequiredScope::RootOnly {
        return false;
    }
    let Some(id) = path.strip_prefix("/api/v1/espacio/spaces/") else {
        return false;
    };
    // Exact match: no sub-path, no trailing slash, no prefix collision.
    if id != own_space_id {
        return false;
    }
    let action = match *method {
        Method::GET | Method::HEAD => SpaceAction::Read,
        Method::DELETE => SpaceAction::Admin,
        _ => return false,
    };
    can(role, action)
}

/// Claims role for a space token. Conservative: only a space Admin maps to
/// `User`; every other role is `Readonly`. Never `Admin`.
pub(crate) fn claims_role_for(role: SpaceRole) -> xavier::security::auth::UserRole {
    match role {
        SpaceRole::Admin => xavier::security::auth::UserRole::User,
        _ => xavier::security::auth::UserRole::Readonly,
    }
}

/// Authenticate and gate a request that carries an `xsp_` token.
///
/// The `SpaceManager` is taken from a request extension installed by the
/// router. When none is installed (the production router mounts it later,
/// WP-13e) every `xsp_` token is refused with 401.
pub(crate) async fn handle_space_token(
    mut req: Request<Body>,
    token: &str,
    next: Next,
) -> Response {
    let unauthorized = || {
        json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"status":"error","message":"Unauthorized"}),
        )
    };
    let forbidden = || {
        json_response(
            StatusCode::FORBIDDEN,
            serde_json::json!({"status":"error","message":"Forbidden"}),
        )
    };
    let Some(manager) = req.extensions().get::<Arc<SpaceManager>>().cloned() else {
        return unauthorized();
    };
    let auth = match xavier::espacio::tokens::verify_with_manager(&manager, token).await {
        Ok(a) => a,
        Err(_) => return unauthorized(),
    };
    // An explicit space header must agree with the token (403, never 404).
    if let Some(h) = req.headers().get("X-Xavier-Space") {
        if h.as_bytes() != auth.space_id.as_bytes() {
            return forbidden();
        }
    }
    if !space_token_may_access(req.uri().path(), &auth.space_id, auth.role, req.method()) {
        return forbidden();
    }
    let claims_role = claims_role_for(auth.role);
    req.extensions_mut().insert(SessionInfo {
        is_ephemeral: false,
        api_token: None,
        user_id: Some(auth.member_id.clone()),
        lease: None,
    });
    req.extensions_mut()
        .insert(xavier::security::auth::Claims::new(
            auth.member_id.clone(),
            "space_token@swal.dev".to_string(),
            claims_role,
            chrono::Duration::hours(1),
        ));
    req.extensions_mut().insert(auth);
    next.run(req).await
}

/// Auth gate for Maloca's mutating endpoints (`/maloca/*`, `/v1/maloca/*`).
///
/// Maloca is intentionally public for reads — GET/HEAD requests pass straight
/// through so the panel and `@swal/maloca-client` dogfood surface keep
/// working without a token, matching the router's existing design. Every
/// other verb (POST/PUT/PATCH/DELETE) is routed through the same
/// [`auth_middleware`] the rest of the API uses, so writes require the root
/// or API token like everything else. OPTIONS also passes through unchanged
/// here; CORS preflight is handled by the `CorsLayer` wrapping this
/// middleware, which short-circuits OPTIONS before it ever reaches us.
pub async fn maloca_mutation_auth_middleware(
    state: State<CliState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }
    auth_middleware(state, req, next).await
}

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
    let is_match: bool = if provided_bytes.len() != expected_bytes.len() {
        let _ = expected_bytes.ct_eq(expected_bytes);
        false
    } else {
        provided_bytes.ct_eq(expected_bytes).into()
    };

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

    // 1b. Space-scoped tokens (WP-13l). Terminal: a token with this prefix is
    // never retried against the other schemes.
    if provided_token_str.starts_with("xsp_") {
        let token = provided_token_str.to_string();
        return handle_space_token(req, &token, next).await;
    }

    // 2. Check Lease Token (F3 - Proxy Authentication)
    if let Some(lease) = state.secrets_engine.get_lease(provided_token_str).await {
        if !lease.is_expired() {
            let path = req.uri().path();
            if !lease_may_access(path) {
                return json_response(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({
                        "status": "error",
                        "message": "Lease tokens are only valid for /v1/proxy"
                    }),
                );
            }
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
            // Scope validation. Deny by default: the requirement is derived
            // from the method and the path, not from a list of allowed
            // paths, so a route nobody classified comes out closed.
            let required = required_scope(req.method(), path);
            if !token_satisfies(&token_meta.scopes, &required) {
                return json_response(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({"status":"error","message":"Insufficient scopes"}),
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

#[cfg(test)]
mod scope_policy_tests {
    use super::{lease_may_access, required_scope, token_satisfies, RequiredScope};
    use axum::http::Method;

    fn scopes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ningun_scope_abre_las_rutas_de_secretos() {
        // `all` used to be the universal key: the old policy granted it to
        // anything that was not a memory route, which included /secrets/*.
        for scope_list in [
            vec!["all"],
            vec!["read", "write", "all"],
            vec!["read", "write", "admin", "secrets", "proxy"],
        ] {
            for path in [
                "/secrets/exec",
                "/secrets/lend",
                "/secrets/leases",
                "/secrets/revoke",
                "/secrets/history",
                "/security/tokens/abc/rotate",
                "/v1/security/approve",
                "/v1/proxy/request",
            ] {
                let required = required_scope(&Method::POST, path);
                assert_eq!(
                    required,
                    RequiredScope::RootOnly,
                    "{path} must be root-only"
                );
                assert!(
                    !token_satisfies(&scopes(&scope_list), &required),
                    "{scope_list:?} must not unlock {path}"
                );
            }
        }
    }

    #[test]
    fn una_ruta_nueva_cae_en_lectura_o_escritura_nunca_abierta() {
        // The regression: a route nobody classified used to inherit
        // `_ => true`. An unknown POST must now demand `write`.
        assert_eq!(
            required_scope(&Method::POST, "/some/route/added/later"),
            RequiredScope::Write
        );
        assert_eq!(
            required_scope(&Method::GET, "/some/route/added/later"),
            RequiredScope::Read
        );
        // A path that merely looks similar is not covered by the secret list.
        assert_eq!(
            required_scope(&Method::POST, "/secretsomething/else"),
            RequiredScope::Write
        );
    }

    #[test]
    fn memoria_conserva_sus_permisos() {
        let read = scopes(&["read"]);
        let write = scopes(&["write"]);
        let all = scopes(&["all"]);
        for path in ["/memory/search", "/v1/memories/search"] {
            // POST search only reads.
            let required = required_scope(&Method::POST, path);
            assert_eq!(required, RequiredScope::Read);
            assert!(token_satisfies(&read, &required));
            assert!(token_satisfies(&all, &required));
        }
        let required = required_scope(&Method::POST, "/memory/add");
        assert_eq!(required, RequiredScope::Write);
        assert!(token_satisfies(&write, &required));
        assert!(token_satisfies(&all, &required));
        assert!(!token_satisfies(&read, &required));
    }

    #[test]
    fn un_lease_solo_sirve_para_el_proxy() {
        assert!(lease_may_access("/v1/proxy/chat/completions"));
        assert!(lease_may_access("/v1/proxy/request"));
        for path in [
            "/secrets/exec",
            "/secrets/lend",
            "/memory/search",
            "/health",
            "/v1/security/approve",
            "/security/tokens",
        ] {
            assert!(!lease_may_access(path), "a lease must not reach {path}");
        }
    }

    /// D2: `/v1/clavis` reads and writes the node's whole secret vault
    /// (JWT_PRIVATE_KEY, DB_MASTER_KEY, node_secret_*), yet it was absent
    /// from `ROOT_ONLY_PREFIXES`. A scoped `xav_` token carrying `all` — the
    /// scope the old `_ => true` arm granted everywhere — therefore cleared
    /// the scope layer and only the role gate stood in front of the vault,
    /// where `/secrets/*` has to agree on three layers.
    #[test]
    fn clavis_es_root_only_para_todos_los_scopes() {
        for scope_list in [
            vec!["all"],
            vec!["read", "write", "all"],
            vec!["read", "write", "admin", "secrets", "proxy"],
        ] {
            for path in ["/v1/clavis/keys/api_key_openai", "/v1/clavis/proxy"] {
                let required = required_scope(&Method::GET, path);
                assert_eq!(
                    required,
                    RequiredScope::RootOnly,
                    "{path} must be root-only"
                );
                // POST is the verb the proxy route and the PUT on keys use.
                assert_eq!(required_scope(&Method::POST, path), RequiredScope::RootOnly);
                assert_eq!(required_scope(&Method::PUT, path), RequiredScope::RootOnly);
                assert!(
                    !token_satisfies(&scopes(&scope_list), &required),
                    "{scope_list:?} must not unlock {path}"
                );
            }
        }
    }

    /// `ROOT_ONLY_PREFIXES` is matched with `starts_with`, and this entry
    /// carries no trailing `/` on purpose: it also covers the bare
    /// `/v1/clavis`, so a route mounted at the root of the Clavis namespace is
    /// root-only without anyone remembering to list it. The cost is that
    /// `/v1/clavisomething` is denied too — no such route exists, and denying
    /// it is the direction that fails safe. Pinned so that switching to
    /// `"/v1/clavis/"` to narrow the match cannot happen by accident: it would
    /// re-open `/v1/clavis` itself.
    #[test]
    fn el_prefijo_clavis_cubre_todo_el_namespace_incluso_el_bare() {
        for path in [
            "/v1/clavis",
            "/v1/clavis/proxy",
            "/v1/clavis/keys/api_key_openai",
            "/v1/clavis/keys/JWT_PRIVATE_KEY",
        ] {
            assert_eq!(
                required_scope(&Method::POST, path),
                RequiredScope::RootOnly,
                "{path} is inside the Clavis namespace and must stay root-only"
            );
        }
    }
}

#[cfg(test)]
mod space_token_tests {
    use super::*;
    use axum::{middleware::from_fn_with_state, routing::any, Extension, Router};
    use tower::ServiceExt;
    use xavier::espacio::tokens::create_space_with_owner_token;
    use xavier::espacio::SpaceContext;

    const OWN: &str = "esp_a";
    const ROOT: &str = "test-token";
    const OWN_PATH: &str = "/api/v1/espacio/spaces/esp_a";

    /// Minimal `CliState` for driving the real `auth_middleware`.
    async fn test_state() -> (CliState, tempfile::TempDir) {
        use crate::cli::state::CodeGraphState;
        use crate::codebase::connection_manager::ConnectionManager;
        use crate::coordination::KeyLendingEngine;
        use crate::secrets::audit::QmdAuditLogger;
        use parking_lot::Mutex;
        use tokio::sync::RwLock as AsyncRwLock;
        use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
        use xavier::agents::rate_limit::RateLimitManager;
        use xavier::app::proxy_use_case::ProxyUseCase;
        use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
        use xavier::app::security_service::SecurityService;
        use xavier::codebase::conversations_db::ConversationsDb;
        use xavier::coordination::{SimpleAgentRegistry, XavierEventBus};
        use xavier::embedding::NoopEmbedder;
        use xavier::memory::agent_indexer::AgentIndexer;
        use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
        use xavier::memory::openclaw_indexer::OpenClawAgentIndexer;
        use xavier::memory::qmd_memory::QmdMemory;
        use xavier::memory::sqlite_vec_store::{
            VecSqliteMemoryStore, VecSqliteStoreConfig, DEFAULT_EMBEDDING_DIMENSIONS,
        };
        use xavier::observability::UsageCounters;
        use xavier::security::sessions::SessionManager;
        use xavier::tasks::store::{InMemoryTaskStore, TaskService};

        let dir = tempfile::tempdir().expect("tempdir");
        let project_id = format!("test_space_token_auth_{}", uuid::Uuid::new_v4());
        ConnectionManager::global()
            .connect_with_path(&project_id, dir.path().join("metrics.db"))
            .expect("isolated metrics db");
        let audit = Box::new(QmdAuditLogger::for_project(&project_id));
        audit.init_schema_async().await.expect("audit schema");
        let engine =
            Arc::new(KeyLendingEngine::new(audit, None).with_leases_project(project_id.clone()));

        let docs = Arc::new(AsyncRwLock::new(Vec::new()));
        let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
        let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));
        let store = Arc::new(
            VecSqliteMemoryStore::new(VecSqliteStoreConfig {
                path: dir.path().join("vec_store.db"),
                embedding_dimensions: DEFAULT_EMBEDDING_DIMENSIONS,
            })
            .await
            .expect("vec store"),
        );
        let cg_db = Arc::new(::code_graph::db::CodeGraphDB::in_memory().expect("code graph db"));
        let cg_state = Arc::new(AsyncRwLock::new(CodeGraphState {
            db: cg_db.clone(),
            indexer: Arc::new(::code_graph::indexer::Indexer::new(cg_db.clone())),
            query: Arc::new(::code_graph::query::QueryEngine::new(cg_db)),
        }));
        let state = CliState {
            memory: memory_port,
            qmd_memory,
            store,
            workspace_id: "test-ws".to_string(),
            workspace_dir: dir.path().to_path_buf(),
            state_dir: dir.path().to_path_buf(),
            auth_db: None,
            code_graph: cg_state,
            security: Arc::new(SecurityService::new()),
            security_scan: Arc::new(SecurityService::new()),
            _time_store: None,
            agent_registry: SimpleAgentRegistry::new(None),
            panel_store: Arc::new(
                ConversationsDb::open_in_memory("test-project")
                    .await
                    .expect("panel store"),
            ),
            secrets_engine: engine,
            event_bus: XavierEventBus::new(10),
            tasks: Arc::new(TaskService::new(Arc::new(InMemoryTaskStore::new()))),
            rate_manager: Arc::new(RateLimitManager::new()),
            prompt_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            http_client: reqwest::Client::new(),
            proxy_use_case: Arc::new(ProxyUseCase::new(
                Arc::new(RateLimitManager::new()),
                Arc::new(Mutex::new(std::collections::HashMap::new())),
            )),
            usage_counters: Arc::new(UsageCounters::new()),
            session_manager: Arc::new(SessionManager::new(60)),
            provider_router: Arc::new(AsyncRwLock::new(ProviderRouter::new(ProviderKind::OpenAI))),
            embedder: Arc::new(NoopEmbedder),
            agent_indexer: Arc::new(AgentIndexer::new(FileIndexer::new(
                FileIndexerConfig::default(),
                None,
            ))),
            auth_store: None,
            openclaw_indexer: Arc::new(OpenClawAgentIndexer::new(Arc::new(NoopEmbedder))),
            multi_db: xavier::storage::multi_db::MultiDbManager::new(),
            system_scan_cache: Arc::new(AsyncRwLock::new(None)),
            maloca: xavier::maloca::MalocaStore::open(&dir.path().join("maloca")),
        };
        (state, dir)
    }

    async fn echo(req: Request<Body>) -> Response {
        let ctx = req.extensions().get::<SpaceContext>().cloned();
        let claims = req
            .extensions()
            .get::<xavier::security::auth::Claims>()
            .map(|c| format!("{:?}", c.role));
        json_response(
            StatusCode::OK,
            serde_json::json!({
                "space": ctx.as_ref().map(|c| c.space_id.clone()),
                "member": ctx.as_ref().map(|c| c.member_id.clone()),
                "role": ctx.as_ref().map(|c| c.role.as_str()),
                "claims_role": claims,
            }),
        )
    }

    /// How the token is presented.
    #[derive(Clone, Copy)]
    enum Hdr {
        Bearer,
        XToken,
    }

    struct Fixture {
        manager: Arc<SpaceManager>,
        app: Router,
        bare: Router,
        admin_a: String,
        admin_b: String,
        _tmp: tempfile::TempDir,
        _tmp_state: tempfile::TempDir,
    }

    async fn fixture() -> Fixture {
        std::env::set_var("XAVIER_TOKEN", ROOT);
        let tmp = tempfile::tempdir().unwrap();
        let manager = Arc::new(SpaceManager::open(tmp.path()));
        let mut raws = Vec::new();
        for (id, owner) in [(OWN, "alice"), ("esp_b", "bob")] {
            let (_, raw) = create_space_with_owner_token(
                &manager,
                id.into(),
                id.into(),
                "".into(),
                owner.into(),
                false,
            )
            .await
            .unwrap();
            raws.push(raw);
        }
        let (state, tmp_state) = test_state().await;
        // The REAL middleware, with the same layering the router uses.
        let build = |with_manager: bool| {
            let r = Router::new()
                .fallback(any(echo))
                .layer(from_fn_with_state(state.clone(), auth_middleware));
            if with_manager {
                r.layer(Extension(manager.clone()))
            } else {
                r
            }
        };
        let app = build(true);
        let bare = build(false);
        let admin_b = raws.pop().unwrap();
        let admin_a = raws.pop().unwrap();
        Fixture {
            manager,
            app,
            bare,
            admin_a,
            admin_b,
            _tmp: tmp,
            _tmp_state: tmp_state,
        }
    }

    async fn send(
        app: &Router,
        method: &str,
        path: &str,
        token: &str,
        hdr: Hdr,
        extra: Option<(&str, &str)>,
    ) -> (StatusCode, serde_json::Value) {
        let mut b = Request::builder().method(method).uri(path);
        if !token.is_empty() {
            b = match hdr {
                Hdr::Bearer => b.header("Authorization", format!("Bearer {token}")),
                Hdr::XToken => b.header("X-Xavier-Token", token),
            };
        }
        if let Some((k, v)) = extra {
            b = b.header(k, v);
        }
        let resp = app
            .clone()
            .oneshot(b.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    async fn call(app: &Router, method: &str, path: &str, token: &str) -> StatusCode {
        send(app, method, path, token, Hdr::Bearer, None).await.0
    }

    fn issue(
        f: &Fixture,
        space: &str,
        member: &str,
        role: SpaceRole,
    ) -> xavier::espacio::tokens::IssuedToken {
        let s = f.manager.stores().get(space).unwrap();
        s.add_member(member, role).unwrap();
        s.issue_token(space, member, role, None).unwrap()
    }

    #[tokio::test]
    async fn real_middleware_accepts_both_header_forms_and_keeps_branch_order() {
        let f = fixture().await;
        for hdr in [Hdr::Bearer, Hdr::XToken] {
            let (st, body) = send(&f.app, "GET", OWN_PATH, &f.admin_a, hdr, None).await;
            assert_eq!(st, StatusCode::OK);
            assert_eq!(body["space"], "esp_a");
            assert_eq!(body["member"], "alice");
            assert_eq!(body["role"], "admin");
            // The root token still wins first and carries no space context.
            let (st, body) = send(&f.app, "GET", "/anything", ROOT, hdr, None).await;
            assert_eq!(st, StatusCode::OK);
            assert!(body["space"].is_null());
            assert_eq!(body["claims_role"], "Admin");
            // A forged xsp_ token is terminal: 401, not retried elsewhere.
            let forged = format!("xsp_esp_a_{}", "A".repeat(43));
            let (st, _) = send(&f.app, "GET", OWN_PATH, &forged, hdr, None).await;
            assert_eq!(st, StatusCode::UNAUTHORIZED);
            // No token / junk token.
            assert_eq!(
                send(&f.app, "GET", OWN_PATH, "", hdr, None).await.0,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                send(&f.app, "GET", OWN_PATH, "junk", hdr, None).await.0,
                StatusCode::UNAUTHORIZED
            );
        }
        // /health stays open and never needs a token.
        assert_eq!(call(&f.app, "GET", "/health", "").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn space_token_is_403_on_workspace_global_memory_and_mcp_routes() {
        let f = fixture().await;
        let tokens = [
            f.admin_a.clone(),
            issue(&f, OWN, "mod", SpaceRole::Moderator).raw,
            issue(&f, OWN, "mem", SpaceRole::Member).raw,
            issue(&f, OWN, "rdr", SpaceRole::Reader).raw,
        ];
        for tok in &tokens {
            for hdr in [Hdr::Bearer, Hdr::XToken] {
                for (m, p) in [
                    ("POST", "/memory/search"),
                    ("GET", "/memory/search"),
                    ("GET", "/memory/export"),
                    ("POST", "/memory/add"),
                    ("GET", "/v1/memories"),
                    ("POST", "/v1/memories"),
                    ("POST", "/v1/memories/search"),
                    ("DELETE", "/v1/memories/x"),
                    ("POST", "/mcp/tools/call"),
                    ("POST", "/mcp"),
                    ("GET", "/mcp"),
                ] {
                    assert_eq!(
                        send(&f.app, m, p, tok, hdr, None).await.0,
                        StatusCode::FORBIDDEN,
                        "{m} {p}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn claims_never_exceed_readonly_except_space_admin_user() {
        let f = fixture().await;
        let cases = [
            (f.admin_a.clone(), "User"),
            (issue(&f, OWN, "mod", SpaceRole::Moderator).raw, "Readonly"),
            (issue(&f, OWN, "mem", SpaceRole::Member).raw, "Readonly"),
            (issue(&f, OWN, "rdr", SpaceRole::Reader).raw, "Readonly"),
        ];
        for (tok, want) in cases {
            let (st, body) = send(&f.app, "GET", OWN_PATH, &tok, Hdr::Bearer, None).await;
            assert_eq!(st, StatusCode::OK);
            assert_eq!(body["claims_role"], want);
            assert_ne!(body["claims_role"], "Admin");
        }
        for r in [SpaceRole::Moderator, SpaceRole::Member, SpaceRole::Reader] {
            assert_eq!(
                claims_role_for(r),
                xavier::security::auth::UserRole::Readonly
            );
        }
        assert_eq!(
            claims_role_for(SpaceRole::Admin),
            xavier::security::auth::UserRole::User
        );
    }

    #[tokio::test]
    async fn token_of_a_is_403_on_b_and_unknown_sub_paths_are_403() {
        let f = fixture().await;
        for (m, p) in [
            ("GET", "/api/v1/espacio/spaces/esp_b"),
            ("DELETE", "/api/v1/espacio/spaces/esp_b"),
            ("GET", "/api/v1/espacio/spaces/esp_nonexistent"),
            ("GET", "/api/v1/espacio/spaces"),
            ("POST", "/api/v1/espacio/spaces"),
            ("GET", "/api/v1/espacio/spaces/"),
            ("GET", "/api/v1/espacio/admin/spaces"),
            ("GET", "/api/v1/espacio/spaces/esp_a/../esp_b"),
            ("GET", "/api/v1/espacio/spaces/esp_a/%2e%2e/esp_b"),
            ("GET", "/api/v1/espacio/spaces/esp_ab"),
            // Unknown sub-paths under the OWN space are denied too.
            ("GET", "/api/v1/espacio/spaces/esp_a/"),
            ("GET", "/api/v1/espacio/spaces/esp_a/members"),
            ("GET", "/api/v1/espacio/spaces/esp_a/tokens"),
            ("GET", "/api/v1/espacio/spaces/esp_a/keys"),
            ("POST", "/api/v1/espacio/spaces/esp_a/messages"),
            ("POST", "/api/v1/espacio/spaces/esp_a/invites"),
            ("GET", "/api/v1/espacio/spaces/esp_a/isolation/esp_b"),
            ("GET", "/api/v1/espacio/spaces/esp_a/whatever/deep/path"),
            // Verbs without a route.
            ("POST", "/api/v1/espacio/spaces/esp_a"),
            ("PUT", "/api/v1/espacio/spaces/esp_a"),
            ("PATCH", "/api/v1/espacio/spaces/esp_a"),
        ] {
            assert_eq!(
                call(&f.app, m, p, &f.admin_a).await,
                StatusCode::FORBIDDEN,
                "{m} {p}"
            );
        }
        assert_eq!(
            call(&f.app, "GET", OWN_PATH, &f.admin_b).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&f.app, "GET", OWN_PATH, &f.admin_a).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn space_header_mismatch_is_403() {
        let f = fixture().await;
        let (st, _) = send(
            &f.app,
            "GET",
            OWN_PATH,
            &f.admin_a,
            Hdr::Bearer,
            Some(("X-Xavier-Space", "esp_b")),
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _) = send(
            &f.app,
            "GET",
            OWN_PATH,
            &f.admin_a,
            Hdr::Bearer,
            Some(("X-Xavier-Space", "esp_a")),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
    }

    #[tokio::test]
    async fn sensitive_and_admin_routes_are_403_even_for_space_admin() {
        let f = fixture().await;
        let mut toks = vec![f.admin_a.clone()];
        for role in [SpaceRole::Moderator, SpaceRole::Member, SpaceRole::Reader] {
            toks.push(issue(&f, OWN, &format!("u_{}", role.as_str()), role).raw);
        }
        for tok in toks {
            for (m, p) in [
                ("GET", "/v1/clavis/keys/api_key_openai"),
                ("POST", "/v1/clavis/proxy"),
                ("GET", "/v1/clavis"),
                ("POST", "/secrets/exec"),
                ("GET", "/secrets/leases"),
                ("POST", "/v1/training/start"),
                ("GET", "/v1/training/jobs"),
                ("GET", "/v1/security/approve"),
                ("POST", "/security/tokens"),
                ("POST", "/v1/proxy/request"),
                ("GET", "/health-ish"),
                ("GET", "/memorybank"),
                ("GET", "/v1/memoriesx"),
            ] {
                assert_eq!(call(&f.app, m, p, &tok).await, StatusCode::FORBIDDEN, "{p}");
            }
        }
    }

    #[tokio::test]
    async fn bad_tokens_are_401() {
        let f = fixture().await;
        let good = "A".repeat(43);
        let secret = f.admin_a.strip_prefix("xsp_esp_a_").unwrap().to_string();
        let s = f.manager.stores().get(OWN).unwrap();
        let revoked = issue(&f, OWN, "rev", SpaceRole::Member);
        s.revoke_token(&revoked.token_id).unwrap();
        let expired = s
            .issue_token(
                OWN,
                "rev",
                SpaceRole::Member,
                Some(chrono::Utc::now() - chrono::Duration::seconds(5)),
            )
            .unwrap();
        let bad: Vec<(&str, String)> = vec![
            ("revoked", revoked.raw),
            ("expired", expired.raw),
            ("forged", format!("xsp_esp_a_{good}")),
            ("wrong-space-prefix", format!("xsp_esp_b_{secret}")),
            ("unknown-space", format!("xsp_nope_{secret}")),
            ("no-secret", "xsp_esp_a_".into()),
            ("short", format!("xsp_esp_a_{}", "A".repeat(42))),
            ("traversal", format!("xsp_../x_{good}")),
            ("prefix-only", "xsp_".into()),
        ];
        for (name, tok) in bad {
            assert_eq!(
                call(&f.app, "GET", OWN_PATH, &tok).await,
                StatusCode::UNAUTHORIZED,
                "{name}"
            );
        }
        // A removed member's token is dead too.
        let gone = issue(&f, OWN, "gone", SpaceRole::Member);
        s.remove_member("gone").unwrap();
        assert_eq!(
            call(&f.app, "GET", OWN_PATH, &gone.raw).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn without_a_space_manager_every_xsp_token_is_401() {
        let f = fixture().await;
        for hdr in [Hdr::Bearer, Hdr::XToken] {
            assert_eq!(
                send(&f.bare, "GET", OWN_PATH, &f.admin_a, hdr, None)
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
        }
        // Root still works on the bare router.
        assert_eq!(call(&f.bare, "GET", "/x", ROOT).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn deleted_space_invalidates_its_tokens() {
        let f = fixture().await;
        f.manager.delete(OWN).await.unwrap();
        assert_eq!(
            call(&f.app, "GET", OWN_PATH, &f.admin_a).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn roles_gate_read_and_delete_on_the_own_space() {
        let f = fixture().await;
        let reader = issue(&f, OWN, "rita", SpaceRole::Reader);
        let member = issue(&f, OWN, "mia", SpaceRole::Member);
        let moderator = issue(&f, OWN, "moe", SpaceRole::Moderator);
        for t in [&reader.raw, &member.raw, &moderator.raw, &f.admin_a] {
            assert_eq!(call(&f.app, "GET", OWN_PATH, t).await, StatusCode::OK);
            assert_eq!(call(&f.app, "HEAD", OWN_PATH, t).await, StatusCode::OK);
        }
        for t in [&reader.raw, &member.raw, &moderator.raw] {
            assert_eq!(
                call(&f.app, "DELETE", OWN_PATH, t).await,
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            call(&f.app, "DELETE", OWN_PATH, &f.admin_a).await,
            StatusCode::OK
        );
    }

    #[test]
    fn allowlist_unit() {
        let g = Method::GET;
        assert!(space_token_may_access(OWN_PATH, OWN, SpaceRole::Reader, &g));
        // The memory allowlist stays removed until WP-13m namespaces it.
        for p in [
            "/memory/search",
            "/memory/export",
            "/v1/memories",
            "/v1/memories/search",
            "/mcp",
            "/mcp/tools/call",
        ] {
            for m in [Method::GET, Method::POST] {
                assert!(!space_token_may_access(p, OWN, SpaceRole::Admin, &m), "{p}");
            }
        }
        assert!(!space_token_may_access(
            "/v1/clavis",
            OWN,
            SpaceRole::Admin,
            &g
        ));
        assert!(!space_token_may_access(
            "memory/search",
            OWN,
            SpaceRole::Admin,
            &g
        ));
        assert!(!space_token_may_access(
            "/api/v1/espacio/spaces/esp_a/members",
            OWN,
            SpaceRole::Admin,
            &g
        ));
        assert!(!space_token_may_access(
            OWN_PATH,
            OWN,
            SpaceRole::Admin,
            &Method::POST
        ));
    }
}
