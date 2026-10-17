//! Marketplace v2 HTTP adapter (MKT-02, publisher-bound mutations in MKT-03).
//!
//! A thin layer over [`xavier_market::MarketStore`]. The routes are mounted only
//! when the crate is built with the `market-v2` feature and the runtime flag
//! `XAVIER_MARKET_V2=beta` is set. With the flag off, [`mount`] returns the
//! router unchanged, so `/v2/marketplace/*` answers 404 and nothing else moves.
//!
//! Listing create, update and revoke need a request signature from the
//! publisher's ed25519 key, sent in the `X-Xavier-Actor-Key`,
//! `X-Xavier-Actor-Timestamp` and `X-Xavier-Actor-Signature` headers. The
//! store only lets that key change its own listings. Reads are unsigned.
//!
//! Config keys (`market.currency`, `market.settlement`, `market.licenses`,
//! `market.legal_text_status`) are read from `XAVIER_MARKET_CURRENCY`,
//! `XAVIER_MARKET_SETTLEMENT`, `XAVIER_MARKET_LICENSES` and
//! `XAVIER_MARKET_LEGAL_TEXT_STATUS`. Unset keys keep their v1 defaults.
//!
//! The store file is `XAVIER_MARKET_V2_DB_PATH` when set, otherwise
//! `market-v2.sqlite3` under `XAVIER_DATA_DIR` (or the working directory).
//!
//! The IVN routes (`/v2/ivn/*`, MKT-17) live in [`ivn`] and share the store.
//! Their config keys are `market.ivn.quorum_ratio` and
//! `market.ivn.validators_per_request`, read from `XAVIER_MARKET_IVN_*`.

pub mod ivn;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Bytes,
    extract::{Extension, Path as PathParam, Query},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use xavier_market::{
    Actor, ListingUpdate, MarketConfig, MarketError, MarketStore, NewListing, NewReceipt,
    SignedRequest,
};

/// Runtime flag. Only the exact value `beta` enables the routes.
pub const FLAG_ENV: &str = "XAVIER_MARKET_V2";
pub const FLAG_VALUE: &str = "beta";
pub const DB_PATH_ENV: &str = "XAVIER_MARKET_V2_DB_PATH";
pub const DB_FILE_NAME: &str = "market-v2.sqlite3";

/// Request headers that carry the publisher signature on mutations.
pub const ACTOR_KEY_HEADER: &str = "x-xavier-actor-key";
pub const ACTOR_TIMESTAMP_HEADER: &str = "x-xavier-actor-timestamp";
pub const ACTOR_SIGNATURE_HEADER: &str = "x-xavier-actor-signature";

/// Whether the beta flag is on. Off unless the value is exactly `beta`.
pub fn is_enabled() -> bool {
    std::env::var(FLAG_ENV).is_ok_and(|value| value.trim() == FLAG_VALUE)
}

/// Adds the marketplace v2 routes when the flag is on and the store opens.
///
/// A store that fails to open is logged and the routes are left out, so a
/// bad beta config cannot take the daemon down.
pub fn mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    if !is_enabled() {
        return router;
    }
    match open_store_from_env() {
        Ok(store) => router.merge(routes(Arc::new(store))),
        Err(err) => {
            tracing::error!(error = %err, "market-v2 is enabled but its store did not open; routes not mounted");
            router
        }
    }
}

/// Opens the store described by the environment.
pub fn open_store_from_env() -> Result<MarketStore, MarketError> {
    let config = MarketConfig::from_lookup(env_config_lookup)?;
    let path = db_path_from_env();
    if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|err| {
            MarketError::InvalidInput(format!("cannot create {}: {err}", parent.display()))
        })?;
    }
    MarketStore::open(&path, config)
}

/// Maps a dotted config key to its env var, e.g. `market.currency` to
/// `XAVIER_MARKET_CURRENCY`.
fn env_config_lookup(key: &str) -> Option<String> {
    let name = format!("XAVIER_{}", key.replace('.', "_").to_uppercase());
    std::env::var(name).ok()
}

fn db_path_from_env() -> PathBuf {
    if let Some(path) = std::env::var_os(DB_PATH_ENV) {
        return PathBuf::from(path);
    }

    let dir = if let Some(d) = std::env::var_os("XAVIER_DATA_DIR") {
        PathBuf::from(d)
    } else {
        dirs::home_dir()
            .map(|h| h.join(".xavier/data"))
            .unwrap_or_else(|| ".".into())
    };

    dir.join(DB_FILE_NAME)
}

/// The `/v2/marketplace/*` routes over a shared store.
pub fn routes<S>(store: Arc<MarketStore>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/v2/marketplace/listings",
            post(create_listing_handler).get(list_listings_handler),
        )
        .route(
            "/v2/marketplace/listings/{id}",
            get(get_listing_handler).patch(update_listing_handler),
        )
        .route(
            "/v2/marketplace/listings/{id}/revoke",
            post(revoke_listing_handler),
        )
        .route(
            "/v2/marketplace/receipts",
            post(create_receipt_handler).get(list_receipts_handler),
        )
        .merge(ivn::routes())
        .layer(Extension(store))
}

type StoreExt = Extension<Arc<MarketStore>>;

async fn create_listing_handler(
    Extension(store): StoreExt,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match authenticate(&method, &uri, &headers, &body) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let new: NewListing = match parse_body(&body) {
        Ok(new) => new,
        Err(response) => return response,
    };
    match store.create_listing(&actor, new) {
        Ok(listing) => (StatusCode::CREATED, Json(listing)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn update_listing_handler(
    Extension(store): StoreExt,
    PathParam(id): PathParam<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match authenticate(&method, &uri, &headers, &body) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let update: ListingUpdate = match parse_body(&body) {
        Ok(update) => update,
        Err(response) => return response,
    };
    match store.update_listing(&actor, &id, update) {
        Ok(listing) => Json(listing).into_response(),
        Err(err) => error_response(err),
    }
}

async fn revoke_listing_handler(
    Extension(store): StoreExt,
    PathParam(id): PathParam<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match authenticate(&method, &uri, &headers, &body) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match store.revoke_listing(&actor, &id) {
        Ok(listing) => Json(listing).into_response(),
        Err(err) => error_response(err),
    }
}

async fn list_listings_handler(Extension(store): StoreExt) -> Response {
    match store.list_listings() {
        Ok(listings) => Json(listings).into_response(),
        Err(err) => error_response(err),
    }
}

async fn get_listing_handler(
    Extension(store): StoreExt,
    PathParam(id): PathParam<String>,
) -> Response {
    match store.get_listing(&id) {
        Ok(Some(listing)) => Json(listing).into_response(),
        Ok(None) => error_status(StatusCode::NOT_FOUND, format!("listing {id} not found")),
        Err(err) => error_response(err),
    }
}

async fn create_receipt_handler(
    Extension(store): StoreExt,
    Json(new): Json<NewReceipt>,
) -> Response {
    match store.record_receipt(new) {
        Ok(receipt) => (StatusCode::CREATED, Json(receipt)).into_response(),
        Err(err) => error_response(err),
    }
}

#[derive(Debug, Deserialize)]
struct BuyerQuery {
    buyer_id: Option<String>,
}

async fn list_receipts_handler(
    Extension(store): StoreExt,
    Query(query): Query<BuyerQuery>,
) -> Response {
    let Some(buyer_id) = query
        .buyer_id
        .filter(|id| !id.trim().is_empty())
        .map(|id| id.trim().to_string())
    else {
        return error_status(
            StatusCode::BAD_REQUEST,
            "buyer_id query parameter is required",
        );
    };
    match store.receipts_for_buyer(&buyer_id) {
        Ok(receipts) => Json(receipts).into_response(),
        Err(err) => error_response(err),
    }
}

/// Verifies the publisher signature on a mutation. The signed path is the
/// request path, and the signed body is the raw request body.
fn authenticate(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Actor, Response> {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    let (Some(public_key_hex), Some(signature_hex), Some(timestamp)) = (
        header(ACTOR_KEY_HEADER),
        header(ACTOR_SIGNATURE_HEADER),
        header(ACTOR_TIMESTAMP_HEADER),
    ) else {
        return Err(error_status(
            StatusCode::UNAUTHORIZED,
            "signed request required: key, timestamp and signature headers",
        ));
    };
    let Ok(timestamp_ms) = timestamp.trim().parse::<i64>() else {
        return Err(error_status(
            StatusCode::UNAUTHORIZED,
            "timestamp header must be unix milliseconds",
        ));
    };
    let request = SignedRequest {
        public_key_hex,
        signature_hex,
        timestamp_ms,
        method: method.as_str(),
        path: uri.path(),
        body,
    };
    Actor::verify(&request, now_ms()).map_err(error_response)
}

fn parse_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, Response> {
    serde_json::from_slice(body).map_err(|err| {
        error_status(
            StatusCode::BAD_REQUEST,
            format!("invalid request body: {err}"),
        )
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

fn error_status(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
        .into_response()
}

fn error_response(err: MarketError) -> Response {
    let status = match &err {
        MarketError::ListingNotFound(_) | MarketError::IdentityRequestNotFound(_) => {
            StatusCode::NOT_FOUND
        }
        MarketError::Unauthenticated(_) => StatusCode::UNAUTHORIZED,
        MarketError::Forbidden(_) => StatusCode::FORBIDDEN,
        MarketError::ListingRevoked(_) | MarketError::Conflict(_) => StatusCode::CONFLICT,
        MarketError::InvalidInput(_)
        | MarketError::LicenseNotEnabled(_)
        | MarketError::Config(_) => StatusCode::BAD_REQUEST,
        MarketError::SchemaTooNew(_) | MarketError::Corrupt(_) | MarketError::Sqlite(_) => {
            tracing::error!(error = %err, "market-v2 store error");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    error_status(status, err.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use axum::{
        body::Body,
        http::{Method, Request},
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::util::ServiceExt;
    use xavier_market::ivn::{public_key_for_seed, sign_vote};
    use xavier_market::{NewValidator, SignedHeaders, Signer, VoteDecision};

    use super::*;

    /// Serialises tests that set process env vars.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Sets the market env vars for one test and restores them on drop.
    struct MarketEnv {
        _lock: MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl MarketEnv {
        fn with_flag(flag: Option<&str>) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            crate::test_support::isolate_and_redirect();
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("market.sqlite3");
            let names = [
                FLAG_ENV,
                DB_PATH_ENV,
                "XAVIER_MARKET_CURRENCY",
                "XAVIER_MARKET_SETTLEMENT",
                "XAVIER_MARKET_LICENSES",
                "XAVIER_MARKET_LEGAL_TEXT_STATUS",
                "XAVIER_MARKET_IVN_QUORUM_RATIO",
                "XAVIER_MARKET_IVN_VALIDATORS_PER_REQUEST",
            ];
            let saved = names
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect();
            for name in names {
                std::env::remove_var(name);
            }
            std::env::set_var(DB_PATH_ENV, &db_path);
            if let Some(flag) = flag {
                std::env::set_var(FLAG_ENV, flag);
            }
            Self {
                _lock: lock,
                _dir: dir,
                saved,
            }
        }
    }

    impl Drop for MarketEnv {
        fn drop(&mut self) {
            for (name, value) in self.saved.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    /// The publisher who owns the listings in these tests.
    fn owner() -> Signer {
        Signer::from_seed([7; 32])
    }

    /// Another identity that tries to mutate the owner's listings.
    fn stranger() -> Signer {
        Signer::from_seed([9; 32])
    }

    fn json_request(method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
        let builder = Request::builder().method(method).uri(uri);
        match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        }
        .expect("build request")
    }

    /// A request signed by `signer` over `uri`'s path and the body as sent.
    fn signed_request(
        signer: &Signer,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> Request<Body> {
        let bytes = body.as_ref().map(Value::to_string).unwrap_or_default();
        let path = uri.split('?').next().unwrap_or(uri);
        let headers = signer.sign(method.as_str(), path, now_ms(), bytes.as_bytes());
        request_with_headers(method, uri, &headers, body)
    }

    fn request_with_headers(
        method: Method,
        uri: &str,
        headers: &SignedHeaders,
        body: Option<Value>,
    ) -> Request<Body> {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(ACTOR_KEY_HEADER, &headers.public_key_hex)
            .header(ACTOR_TIMESTAMP_HEADER, headers.timestamp_ms.to_string())
            .header(ACTOR_SIGNATURE_HEADER, &headers.signature_hex);
        match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        }
        .expect("build request")
    }

    async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = router.clone().oneshot(request).await.expect("route");
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, body)
    }

    fn listing_body() -> Value {
        json!({
            "seller_id": owner().public_key_hex(),
            "title": "Weather station logs",
            "license": "CC-BY-4.0",
            "price_credits": 25
        })
    }

    /// Creates one listing as the owner and returns its id.
    async fn create_owned_listing(router: &Router) -> String {
        let (status, listing) = send(
            router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/listings",
                Some(listing_body()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        listing["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn flag_off_by_default_mounts_no_routes() {
        let _env = MarketEnv::with_flag(None);
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/listings",
                Some(listing_body()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_the_exact_beta_value_enables_the_routes() {
        for value in ["on", "true", "BETA-1", ""] {
            let _env = MarketEnv::with_flag(Some(value));
            let router: Router = mount(Router::new());
            let (status, _) = send(
                &router,
                json_request(Method::GET, "/v2/marketplace/listings", None),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "flag value {value:?}");
        }
        // Positive control: the exact value mounts the routes, so dropping the
        // `is_enabled` guard turns the loop above red.
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/listings", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn unsigned_mutations_are_refused() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;
        let listing_uri = format!("/v2/marketplace/listings/{listing_id}");
        let revoke_uri = format!("{listing_uri}/revoke");

        let (status, _) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/listings",
                Some(listing_body()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let patch = json!({"price_credits": 1});
        let (status, _) = send(
            &router,
            json_request(Method::PATCH, &listing_uri, Some(patch)),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _) = send(&router, json_request(Method::POST, &revoke_uri, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // The listing is untouched by the refused calls.
        let (status, listing) = send(&router, json_request(Method::GET, &listing_uri, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listing["price_credits"], 25);
        assert_eq!(listing["revoked_at_ms"], Value::Null);
    }

    #[tokio::test]
    async fn another_identity_cannot_mutate_and_the_owner_can() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;
        let listing_uri = format!("/v2/marketplace/listings/{listing_id}");
        let revoke_uri = format!("{listing_uri}/revoke");

        let (status, _) = send(
            &router,
            signed_request(
                &stranger(),
                Method::PATCH,
                &listing_uri,
                Some(json!({"price_credits": 1})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _) = send(
            &router,
            signed_request(&stranger(), Method::POST, &revoke_uri, None),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // The owner's own calls succeed.
        let (status, updated) = send(
            &router,
            signed_request(
                &owner(),
                Method::PATCH,
                &listing_uri,
                Some(json!({"price_credits": 30})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["price_credits"], 30);

        let (status, revoked) = send(
            &router,
            signed_request(&owner(), Method::POST, &revoke_uri, None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(revoked["revoked_at_ms"].is_i64());

        // A revoked listing cannot be bought.
        let (status, _) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/receipts",
                Some(json!({"listing_id": listing_id, "buyer_id": "buyer-1"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn create_refuses_a_seller_id_that_is_not_the_signer() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            signed_request(
                &stranger(),
                Method::POST,
                "/v2/marketplace/listings",
                Some(listing_body()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (_, listings) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/listings", None),
        )
        .await;
        assert_eq!(listings, json!([]));
    }

    #[tokio::test]
    async fn tampered_replayed_or_stale_signatures_are_refused() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;
        let listing_uri = format!("/v2/marketplace/listings/{listing_id}");

        // Body changed after signing.
        let signed_for_one = owner().sign(
            "PATCH",
            &listing_uri,
            now_ms(),
            json!({"price_credits": 1}).to_string().as_bytes(),
        );
        let (status, _) = send(
            &router,
            request_with_headers(
                Method::PATCH,
                &listing_uri,
                &signed_for_one,
                Some(json!({"price_credits": 999})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Signature for one listing replayed against another.
        let other_uri = "/v2/marketplace/listings/someone-else/revoke";
        let signed_for_revoke =
            owner().sign("POST", &format!("{listing_uri}/revoke"), now_ms(), b"");
        let (status, _) = send(
            &router,
            request_with_headers(Method::POST, other_uri, &signed_for_revoke, None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Timestamp outside the skew window.
        let ten_minutes_ago = now_ms() - 10 * 60 * 1000;
        let stale = owner().sign("POST", "/v2/marketplace/listings", ten_minutes_ago, b"");
        let (status, _) = send(
            &router,
            request_with_headers(Method::POST, "/v2/marketplace/listings", &stale, None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Malformed timestamp header.
        let malformed = Request::builder()
            .method(Method::POST)
            .uri("/v2/marketplace/listings")
            .header(ACTOR_KEY_HEADER, owner().public_key_hex())
            .header(ACTOR_TIMESTAMP_HEADER, "yesterday")
            .header(ACTOR_SIGNATURE_HEADER, "00")
            .body(Body::empty())
            .unwrap();
        let (status, _) = send(&router, malformed).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // The listing still has its original price.
        let (_, listing) = send(&router, json_request(Method::GET, &listing_uri, None)).await;
        assert_eq!(listing["price_credits"], 25);
    }

    #[tokio::test]
    async fn client_errors_map_to_4xx() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());

        let mut free_text = listing_body();
        free_text["license"] = json!("MIT");
        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/listings",
                Some(free_text),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::PATCH,
                "/v2/marketplace/listings/missing",
                Some(json!({"price_credits": 1})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, body) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/receipts",
                Some(json!({"listing_id": "missing", "buyer_id": "buyer-1"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].as_str().unwrap().contains("missing"));

        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/receipts", None),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn bad_config_leaves_routes_unmounted_instead_of_crashing() {
        let _env = MarketEnv::with_flag(Some("beta"));
        std::env::set_var("XAVIER_MARKET_SETTLEMENT", "on_chain");
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/listings", None),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    fn ivn_seed(index: u8) -> [u8; 32] {
        [index + 1; 32]
    }

    #[tokio::test]
    async fn ivn_routes_are_off_without_the_flag() {
        let _env = MarketEnv::with_flag(None);
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/ivn/requests",
                Some(json!({"applicant": "applicant-1"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ivn_state_survives_a_restart_and_unsigned_votes_are_refused() {
        let _env = MarketEnv::with_flag(Some("beta"));
        {
            // HTTP cannot register validators, so the registry is filled through
            // the store that the mount reopens below.
            let store = open_store_from_env().unwrap();
            for index in 0..5u8 {
                store
                    .register_validator(NewValidator {
                        node_id: format!("validator-{index}"),
                        public_key: public_key_for_seed(&ivn_seed(index)),
                    })
                    .unwrap();
            }
        }

        let router: Router = mount(Router::new());
        let (status, created) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/ivn/requests",
                Some(json!({"applicant": "applicant-1"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let request_id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["validators"].as_array().unwrap().len(), 5);
        let request_uri = format!("/v2/ivn/requests/{request_id}");
        let vote_uri = format!("{request_uri}/votes");

        let unsigned = json!({
            "validator_id": "validator-0",
            "decision": "check",
            "signature": ""
        });
        let (status, _) = send(
            &router,
            json_request(Method::POST, &vote_uri, Some(unsigned)),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (_, request) = send(&router, json_request(Method::GET, &request_uri, None)).await;
        assert_eq!(request["votes"], json!([]));

        let signature = sign_vote(
            &ivn_seed(0),
            &request_id,
            "validator-0",
            VoteDecision::Check,
        );
        let signed = json!({
            "validator_id": "validator-0",
            "decision": "check",
            "signature": signature
        });
        let (status, _) = send(&router, json_request(Method::POST, &vote_uri, Some(signed))).await;
        assert_eq!(status, StatusCode::CREATED);

        // A fresh mount reopens the same file, as a daemon restart does.
        drop(router);
        let restarted: Router = mount(Router::new());
        let (status, request) =
            send(&restarted, json_request(Method::GET, &request_uri, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(request["votes"].as_array().unwrap().len(), 1);
        assert_eq!(request["votes"][0]["validator_id"], json!("validator-0"));
    }

    #[tokio::test]
    async fn config_keys_map_to_env_vars_and_limit_licenses() {
        let _env = MarketEnv::with_flag(Some("beta"));
        std::env::set_var("XAVIER_MARKET_LICENSES", "CC-BY-NC-4.0");
        let router: Router = mount(Router::new());

        // CC-BY-4.0 is not in the configured subset, so the create is refused.
        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/listings",
                Some(listing_body()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
