//! Marketplace v2 HTTP adapter (MKT-02).
//!
//! A thin layer over [`xavier_market::MarketStore`]. The routes are mounted only
//! when the crate is built with the `market-v2` feature and the runtime flag
//! `XAVIER_MARKET_V2=beta` is set. With the flag off, [`mount`] returns the
//! router unchanged, so `/v2/marketplace/*` answers 404 and nothing else moves.
//!
//! Config keys (`market.currency`, `market.settlement`, `market.licenses`,
//! `market.legal_text_status`) are read from `XAVIER_MARKET_CURRENCY`,
//! `XAVIER_MARKET_SETTLEMENT`, `XAVIER_MARKET_LICENSES` and
//! `XAVIER_MARKET_LEGAL_TEXT_STATUS`. Unset keys keep their v1 defaults.
//!
//! The store file is `XAVIER_MARKET_V2_DB_PATH` when set, otherwise
//! `market-v2.sqlite3` under `XAVIER_DATA_DIR` (or the working directory).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Extension, Path as PathParam, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use xavier_market::{MarketConfig, MarketError, MarketStore, NewListing, NewReceipt};

/// Runtime flag. Only the exact value `beta` enables the routes.
pub const FLAG_ENV: &str = "XAVIER_MARKET_V2";
pub const FLAG_VALUE: &str = "beta";
pub const DB_PATH_ENV: &str = "XAVIER_MARKET_V2_DB_PATH";
pub const DB_FILE_NAME: &str = "market-v2.sqlite3";

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
        .route("/v2/marketplace/listings/{id}", get(get_listing_handler))
        .route(
            "/v2/marketplace/receipts",
            post(create_receipt_handler).get(list_receipts_handler),
        )
        .layer(Extension(store))
}

type StoreExt = Extension<Arc<MarketStore>>;

async fn create_listing_handler(
    Extension(store): StoreExt,
    Json(new): Json<NewListing>,
) -> Response {
    match store.create_listing(new) {
        Ok(listing) => (StatusCode::CREATED, Json(listing)).into_response(),
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
        MarketError::ListingNotFound(_) => StatusCode::NOT_FOUND,
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
            "seller_id": "seller-1",
            "title": "Weather station logs",
            "license": "CC-BY-4.0",
            "price_credits": 25
        })
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
    async fn client_errors_map_to_4xx() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());

        let mut free_text = listing_body();
        free_text["license"] = json!("MIT");
        let (status, _) = send(
            &router,
            json_request(Method::POST, "/v2/marketplace/listings", Some(free_text)),
        )
        .await;
        assert!(status.is_client_error(), "free-text license got {status}");

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

    #[tokio::test]
    async fn config_keys_map_to_env_vars_and_limit_licenses() {
        let _env = MarketEnv::with_flag(Some("beta"));
        std::env::set_var("XAVIER_MARKET_LICENSES", "CC-BY-NC-4.0");
        let router: Router = mount(Router::new());

        // CC-BY-4.0 is not in the configured subset, so the create is refused.
        let (status, _) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/listings",
                Some(listing_body()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
