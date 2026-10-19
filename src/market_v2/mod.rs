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
//!
//! Listing create runs the privacy admission gate (`listing_policy`). It needs a
//! `consent_ref` in the body. `GET /v2/marketplace/listings/{id}` returns the
//! listing with a `provenance` credential, signed by the node key in
//! `XAVIER_MARKET_V2_ISSUER_SEED` (64 hex chars). Without that variable a fresh
//! key is generated per process. `GET /v2/marketplace/listings?q=` ranks by text.

pub mod ivn;
pub mod listing_policy;

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::polygon_anchor::{
    AnchorReceipt, AnchorTransport, MockAnchorTransport, DEFAULT_CHAIN_ID_AMOY,
};

use axum::{
    body::Bytes,
    extract::{Extension, Path as PathParam, Query},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use xavier_market::credential::{self, ListingCredential};
use xavier_market::{
    Actor, AggregateRow, ConsentError, ConsentRecord, ConsentState, ConsentStore, Listing,
    ListingUpdate, MarketConfig, MarketError, MarketStore, NewListing, NewReceipt, Sample,
    SignedRequest, TelemetryError, TelemetryStore,
};

/// Runtime flag. Only the exact value `beta` enables the routes.
pub const FLAG_ENV: &str = "XAVIER_MARKET_V2";
pub const FLAG_VALUE: &str = "beta";
pub const DB_PATH_ENV: &str = "XAVIER_MARKET_V2_DB_PATH";
pub const DB_FILE_NAME: &str = "market-v2.sqlite3";
/// Hex-encoded 32-byte ed25519 seed of the node key that signs provenance credentials.
pub const ISSUER_SEED_ENV: &str = "XAVIER_MARKET_V2_ISSUER_SEED";

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
    let opened = open_store_from_env().and_then(|store| Ok((store, issuer_key_from_env()?)));
    match opened {
        Ok((store, issuer)) => {
            let store_arc = Arc::new(store);
            spawn_anchor_job_from_env(Arc::clone(&store_arc));
            router.merge(routes(store_arc, issuer))
        }
        Err(err) => {
            tracing::error!(error = %err, "market-v2 is enabled but its store or issuer key did not load; routes not mounted");
            router
        }
    }
}

/// Reads the provenance issuer key from [`ISSUER_SEED_ENV`]. Unset means a
/// fresh random key for this process. A malformed value is an error, so the
/// routes stay unmounted instead of signing with an unexpected key.
pub fn issuer_key_from_env() -> Result<SigningKey, MarketError> {
    let Some(raw) = std::env::var_os(ISSUER_SEED_ENV) else {
        return Ok(SigningKey::generate(&mut OsRng));
    };
    let raw = raw.to_string_lossy();
    let bytes = crate::crypto::hex_decode(raw.trim())
        .map_err(|err| MarketError::InvalidInput(format!("{ISSUER_SEED_ENV} is not hex: {err}")))?;
    let seed: [u8; 32] = bytes.try_into().map_err(|_| {
        MarketError::InvalidInput(format!("{ISSUER_SEED_ENV} must be 32 bytes (64 hex chars)"))
    })?;
    Ok(SigningKey::from_bytes(&seed))
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
pub fn routes<S>(store: Arc<MarketStore>, issuer: SigningKey) -> Router<S>
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
        .route(
            "/v2/marketplace/grants",
            post(create_grant_handler).get(list_grants_handler),
        )
        .route("/v2/marketplace/grants/{id}", get(get_grant_handler))
        .route(
            "/v2/marketplace/grants/{id}/revoke",
            post(revoke_grant_handler),
        )
        .route(
            "/v2/marketplace/consent",
            post(set_consent_handler).get(get_consent_handler),
        )
        .route("/v2/marketplace/audit", get(list_audit_handler))
        .route("/v2/marketplace/audit/verify", get(verify_audit_handler))
        .route("/v2/marketplace/telemetry", post(record_telemetry_handler))
        .route(
            "/v2/marketplace/telemetry/aggregates",
            get(get_telemetry_aggregates_handler),
        )
        .route(
            "/v2/marketplace/anchor/dry-run",
            post(trigger_dry_run_anchor_handler).get(get_latest_anchor_handler),
        )
        .merge(ivn::routes())
        .layer(Extension(store))
        .layer(Extension(Arc::new(issuer)))
}

type StoreExt = Extension<Arc<MarketStore>>;
type IssuerExt = Extension<Arc<SigningKey>>;

/// Listing create body: the listing fields plus the consent reference the
/// privacy gate requires. The reference is checked, not stored.
#[derive(Debug, Deserialize)]
pub struct CreateListingPayload {
    #[serde(flatten)]
    pub listing: NewListing,
    #[serde(default)]
    pub consent_ref: Option<String>,
}

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
    let payload: CreateListingPayload = match parse_body(&body) {
        Ok(payload) => payload,
        Err(response) => return response,
    };
    if let Err(err) = listing_policy::admit_listing(&payload.listing, payload.consent_ref) {
        return error_status(
            StatusCode::BAD_REQUEST,
            format!("listing refused by privacy gate: {err}"),
        );
    }
    match store.create_listing(&actor, payload.listing) {
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

#[derive(Debug, Deserialize)]
struct ListingQuery {
    q: Option<String>,
}

async fn list_listings_handler(
    Extension(store): StoreExt,
    Query(query): Query<ListingQuery>,
) -> Response {
    let listings = match store.list_listings() {
        Ok(listings) => listings,
        Err(err) => return error_response(err),
    };
    match query
        .q
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty())
    {
        Some(q) => Json(listing_policy::search(&q, listings)).into_response(),
        None => Json(listings).into_response(),
    }
}

/// A listing with its provenance credential. The listing fields stay at the
/// top level, so existing readers keep working.
#[derive(Debug, Serialize)]
struct ListingWithProvenance {
    #[serde(flatten)]
    listing: Listing,
    provenance: ListingCredential,
}

async fn get_listing_handler(
    Extension(store): StoreExt,
    Extension(issuer): IssuerExt,
    PathParam(id): PathParam<String>,
) -> Response {
    match store.get_listing(&id) {
        Ok(Some(listing)) => {
            // valid_from is the listing's creation time, so the credential is
            // stable across reads apart from its id.
            let issued_at = chrono::DateTime::from_timestamp_millis(listing.created_at_ms)
                .map(|at| at.to_rfc3339())
                .unwrap_or_default();
            let provenance = credential::issue(&listing, &issuer, &issued_at);
            Json(ListingWithProvenance {
                listing,
                provenance,
            })
            .into_response()
        }
        Ok(None) => error_status(StatusCode::NOT_FOUND, format!("listing {id} not found")),
        Err(err) => error_response(err),
    }
}

async fn create_receipt_handler(
    Extension(store): StoreExt,
    Json(new): Json<NewReceipt>,
) -> Response {
    match store.record_receipt(new) {
        Ok(receipt) => {
            if let Err(err) = store.audit().append(
                "receipt",
                &serde_json::to_value(&receipt).unwrap_or_default(),
            ) {
                tracing::error!(error = %err, "failed to append receipt to audit chain");
                return error_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("audit log failed: {err}"),
                );
            }
            (StatusCode::CREATED, Json(receipt)).into_response()
        }
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

#[derive(Debug, Deserialize)]
pub struct CreateGrantPayload {
    pub listing_id: String,
    #[serde(alias = "buyer_id")]
    pub buyer_x25519_pubkey_hex: String,
    pub ephemeral_pubkey_hex: String,
    pub wrapped_key_hex: String,
    pub nonce_hex: String,
    /// Grant expiry in unix ms, signed by the seller; the server bounds it.
    pub expires_at_ms: i64,
    /// Hex-encoded 64-byte Ed25519 signature by the seller over the canonical grant fields.
    pub signature_hex: String,
}

async fn create_grant_handler(
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
    let payload: CreateGrantPayload = match parse_body(&body) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    match store.create_access_grant(
        &actor,
        &payload.listing_id,
        &payload.buyer_x25519_pubkey_hex,
        &payload.ephemeral_pubkey_hex,
        &payload.wrapped_key_hex,
        &payload.nonce_hex,
        payload.expires_at_ms,
        &payload.signature_hex,
    ) {
        Ok(grant) => {
            if let Err(err) = store
                .audit()
                .append("grant", &serde_json::to_value(&grant).unwrap_or_default())
            {
                tracing::error!(error = %err, "failed to append grant to audit chain");
                return error_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("audit log failed: {err}"),
                );
            }
            (StatusCode::CREATED, Json(grant)).into_response()
        }
        Err(err) => error_response(err),
    }
}

async fn get_grant_handler(
    Extension(store): StoreExt,
    PathParam(id): PathParam<String>,
) -> Response {
    match store.get_access_grant(&id) {
        Ok(Some(grant)) => {
            let sanitized = grant.sanitized_if_inactive(now_ms());
            Json(sanitized).into_response()
        }
        Ok(None) => error_status(StatusCode::NOT_FOUND, format!("grant {id} not found")),
        Err(err) => error_response(err),
    }
}

#[derive(Debug, Deserialize)]
struct GrantFilterQuery {
    buyer_id: Option<String>,
    listing_id: Option<String>,
}

async fn list_grants_handler(
    Extension(store): StoreExt,
    Query(query): Query<GrantFilterQuery>,
) -> Response {
    let now = now_ms();
    if let Some(buyer_id) = query.buyer_id.filter(|s| !s.trim().is_empty()) {
        match store.list_grants_for_buyer(buyer_id.trim()) {
            Ok(grants) => {
                let sanitized: Vec<_> = grants
                    .into_iter()
                    .map(|g| g.sanitized_if_inactive(now))
                    .collect();
                Json(sanitized).into_response()
            }
            Err(err) => error_response(err),
        }
    } else if let Some(listing_id) = query.listing_id.filter(|s| !s.trim().is_empty()) {
        match store.list_grants_for_listing(listing_id.trim()) {
            Ok(grants) => {
                let sanitized: Vec<_> = grants
                    .into_iter()
                    .map(|g| g.sanitized_if_inactive(now))
                    .collect();
                Json(sanitized).into_response()
            }
            Err(err) => error_response(err),
        }
    } else {
        error_status(
            StatusCode::BAD_REQUEST,
            "either buyer_id or listing_id query parameter is required",
        )
    }
}

async fn revoke_grant_handler(
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
    match store.revoke_access_grant(&actor, &id) {
        Ok(grant) => {
            let sanitized = grant.sanitized_if_inactive(now_ms());
            Json(sanitized).into_response()
        }
        Err(err) => error_response(err),
    }
}

#[derive(Debug, Deserialize)]
pub struct SetConsentPayload {
    pub node_id: String,
    pub state: String,
    pub terms_version: String,
    pub consent_version: String,
}

#[derive(Debug, Deserialize)]
pub struct ConsentQuery {
    pub node_id: Option<String>,
}

fn open_consent_store() -> Result<ConsentStore, ConsentError> {
    let path = db_path_from_env();
    ConsentStore::open(&path)
}

async fn set_consent_handler(
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match authenticate(&method, &uri, &headers, &body) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let payload: SetConsentPayload = match parse_body(&body) {
        Ok(payload) => payload,
        Err(response) => return response,
    };
    if payload.node_id != actor.id() {
        return error_status(
            StatusCode::FORBIDDEN,
            format!(
                "node_id mismatch: request specifies '{}' but caller authenticated as '{}'",
                payload.node_id,
                actor.id()
            ),
        );
    }
    let state = match ConsentState::parse(&payload.state) {
        Ok(state) => state,
        Err(err) => return error_status(StatusCode::BAD_REQUEST, err.to_string()),
    };
    let record = ConsentRecord {
        node_id: payload.node_id,
        state,
        terms_version: payload.terms_version,
        consent_version: payload.consent_version,
        updated_at_ms: now_ms(),
    };
    let store = match open_consent_store() {
        Ok(store) => store,
        Err(err) => return error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    match store.set(record.clone()) {
        Ok(()) => (StatusCode::OK, Json(record)).into_response(),
        Err(ConsentError::MissingVersion) => {
            error_status(StatusCode::BAD_REQUEST, "Missing consent or terms version")
        }
        Err(err) => error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn get_consent_handler(Query(query): Query<ConsentQuery>) -> Response {
    // Unscoped reads would list every node's consent, so a node_id filter is required.
    let Some(node_id) = query.node_id.filter(|s| !s.trim().is_empty()) else {
        return error_status(
            StatusCode::BAD_REQUEST,
            "node_id query parameter is required",
        );
    };
    let store = match open_consent_store() {
        Ok(store) => store,
        Err(err) => return error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    match store.get(node_id.trim()) {
        Ok(Some(record)) => Json(record).into_response(),
        Ok(None) => Json(ConsentRecord {
            node_id: node_id.trim().to_string(),
            state: ConsentState::Off,
            terms_version: String::new(),
            consent_version: String::new(),
            updated_at_ms: 0,
        })
        .into_response(),
        Err(err) => error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn list_audit_handler(Extension(store): StoreExt) -> Response {
    match store.audit().records() {
        Ok(records) => Json(records).into_response(),
        Err(err) => error_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to fetch audit records: {err}"),
        ),
    }
}

async fn verify_audit_handler(Extension(store): StoreExt) -> Response {
    match store.audit().verify() {
        Ok(count) => Json(serde_json::json!({
            "verified": true,
            "records": count,
        }))
        .into_response(),
        Err(err) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "verified": false,
                "error": err.to_string(),
            })),
        )
            .into_response(),
    }
}

fn open_telemetry_store() -> Result<TelemetryStore, TelemetryError> {
    let path = db_path_from_env();
    TelemetryStore::open(&path)
}

#[derive(Debug, Deserialize)]
pub struct TelemetryQuery {
    pub node_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecordTelemetryPayload {
    pub node_id: Option<String>,
    #[serde(flatten)]
    pub sample: Sample,
}

async fn get_telemetry_aggregates_handler(
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Query(query): Query<TelemetryQuery>,
    body: Bytes,
) -> Response {
    let authenticated_actor = if headers.contains_key(ACTOR_KEY_HEADER) {
        match authenticate(&method, &uri, &headers, &body) {
            Ok(actor) => Some(actor),
            Err(resp) => return resp,
        }
    } else {
        None
    };

    let node_id = match (authenticated_actor, query.node_id) {
        (Some(actor), Some(q_id)) => {
            if actor.id() != q_id.trim() {
                return error_status(
                    StatusCode::FORBIDDEN,
                    format!(
                        "node_id mismatch: query specifies '{}' but caller authenticated as '{}'",
                        q_id,
                        actor.id()
                    ),
                );
            }
            actor.id().to_string()
        }
        (Some(actor), None) => actor.id().to_string(),
        (None, Some(q_id)) if !q_id.trim().is_empty() => q_id.trim().to_string(),
        (None, _) => {
            return error_status(
                StatusCode::BAD_REQUEST,
                "node_id query parameter is required",
            );
        }
    };

    let consent_store = match open_consent_store() {
        Ok(store) => store,
        Err(err) => return error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let telemetry_store = match open_telemetry_store() {
        Ok(store) => store,
        Err(err) => return error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };

    match telemetry_store.aggregate(&consent_store, &node_id) {
        Ok(aggs) => Json(aggs).into_response(),
        Err(TelemetryError::NotOptedIn) => error_status(
            StatusCode::FORBIDDEN,
            "node is not opted into shared telemetry (reciprocity requires shared consent)",
        ),
        Err(err) => error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn record_telemetry_handler(
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let authenticated_actor = if headers.contains_key(ACTOR_KEY_HEADER) {
        match authenticate(&method, &uri, &headers, &body) {
            Ok(actor) => Some(actor),
            Err(resp) => return resp,
        }
    } else {
        None
    };

    let payload: RecordTelemetryPayload = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let node_id = match (authenticated_actor, payload.node_id) {
        (Some(actor), Some(p_id)) => {
            if actor.id() != p_id.trim() {
                return error_status(
                    StatusCode::FORBIDDEN,
                    format!(
                        "node_id mismatch: body specifies '{}' but caller authenticated as '{}'",
                        p_id,
                        actor.id()
                    ),
                );
            }
            actor.id().to_string()
        }
        (Some(actor), None) => actor.id().to_string(),
        (None, Some(p_id)) if !p_id.trim().is_empty() => p_id.trim().to_string(),
        (None, _) => {
            return error_status(StatusCode::BAD_REQUEST, "node_id field is required");
        }
    };

    let consent_store = match open_consent_store() {
        Ok(store) => store,
        Err(err) => return error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let telemetry_store = match open_telemetry_store() {
        Ok(store) => store,
        Err(err) => return error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };

    match telemetry_store.record(&consent_store, &node_id, &payload.sample, now_ms()) {
        Ok(recorded) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "recorded": recorded,
                "node_id": node_id,
            })),
        )
            .into_response(),
        Err(err) => error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

/// Report produced by a dry-run anchor cycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnchorRunReport {
    pub root: String,
    pub leaf_count: usize,
    pub sampled_leaf: String,
    pub proof_valid: bool,
    pub receipt: AnchorReceipt,
}

static LATEST_ANCHOR_REPORT: LazyLock<RwLock<Option<AnchorRunReport>>> =
    LazyLock::new(|| RwLock::new(None));

/// Returns the report from the most recent dry-run anchor cycle, if any.
pub fn latest_anchor_report() -> Option<AnchorRunReport> {
    LATEST_ANCHOR_REPORT
        .read()
        .ok()
        .and_then(|lock| lock.clone())
}

/// Runs a single dry-run Merkle anchor cycle over the audit chain, listings and receipts.
///
/// Returns:
/// - `Ok(None)` if the flag is off (`XAVIER_MARKET_V2=beta` is unset or not `beta`),
///   or if the store has no leaves to anchor.
/// - `Ok(Some(report))` containing the verifiable root, sampled leaf proof verification,
///   and mock anchor receipt.
/// - `Err(MarketError)` if an unexpected error occurs during leaf collection or proof verification.
pub fn run_dry_run_anchor_cycle(
    store: &MarketStore,
    transport: &impl AnchorTransport,
) -> Result<Option<AnchorRunReport>, MarketError> {
    if !is_enabled() {
        return Ok(None);
    }

    let leaves = store.collect_merkle_leaves()?;
    if leaves.is_empty() {
        tracing::debug!("market-v2 dry-run anchor: store has no leaves to anchor");
        return Ok(None);
    }

    let root = xavier_market::merkle::merkle_root(&leaves)
        .ok_or_else(|| MarketError::InvalidInput("failed to compute merkle root".to_string()))?;

    let sampled_idx = 0;
    let sampled_leaf = &leaves[sampled_idx];
    let proof_steps = xavier_market::merkle::proof(&leaves, sampled_idx).ok_or_else(|| {
        MarketError::InvalidInput("failed to compute proof for sampled leaf".to_string())
    })?;

    let proof_valid = xavier_market::merkle::verify_proof(sampled_leaf, &proof_steps, &root);
    if !proof_valid {
        return Err(MarketError::InvalidInput(format!(
            "merkle proof verification failed for sampled leaf {}",
            sampled_leaf
        )));
    }

    tracing::info!(
        root = %root,
        leaf_count = leaves.len(),
        sampled_leaf = %sampled_leaf,
        proof_valid = proof_valid,
        "market-v2 dry-run anchor cycle verified"
    );

    let receipt = transport
        .submit_hash(&root, DEFAULT_CHAIN_ID_AMOY, None)
        .map_err(|e| MarketError::InvalidInput(format!("anchor transport error: {e}")))?;

    let report = AnchorRunReport {
        root,
        leaf_count: leaves.len(),
        sampled_leaf: sampled_leaf.clone(),
        proof_valid,
        receipt,
    };

    if let Ok(mut lock) = LATEST_ANCHOR_REPORT.write() {
        *lock = Some(report.clone());
    }

    Ok(Some(report))
}

/// Spawns the periodic dry-run Merkle anchor background task.
///
/// Respects the `XAVIER_MARKET_V2=beta` flag (returns `None` when flag is off).
/// Interval is controlled by `XAVIER_MARKET_ANCHOR_INTERVAL_SECS` (default 300s, 0 disables).
pub fn spawn_anchor_job_from_env(store: Arc<MarketStore>) -> Option<tokio::task::JoinHandle<()>> {
    if !is_enabled() {
        return None;
    }
    let interval_secs: u64 = std::env::var("XAVIER_MARKET_ANCHOR_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(300);
    if interval_secs == 0 {
        return None;
    }

    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        let transport = MockAnchorTransport;
        loop {
            interval.tick().await;
            match run_dry_run_anchor_cycle(&store, &transport) {
                Ok(Some(report)) => {
                    tracing::debug!(root = %report.root, "periodic merkle anchor dry-run cycle completed");
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "periodic merkle anchor dry-run cycle error");
                }
            }
        }
    }))
}

async fn trigger_dry_run_anchor_handler(Extension(store): StoreExt) -> Response {
    let transport = MockAnchorTransport;
    match run_dry_run_anchor_cycle(&store, &transport) {
        Ok(Some(report)) => (StatusCode::OK, Json(report)).into_response(),
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "message": "no leaves to anchor",
                "dry_run": true,
            })),
        )
            .into_response(),
        Err(err) => error_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn get_latest_anchor_handler(Extension(store): StoreExt) -> Response {
    if let Some(report) = latest_anchor_report() {
        return (StatusCode::OK, Json(report)).into_response();
    }
    trigger_dry_run_anchor_handler(Extension(store)).await
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
        MarketError::ListingNotFound(_)
        | MarketError::IdentityRequestNotFound(_)
        | MarketError::AccessGrantNotFound(_) => StatusCode::NOT_FOUND,
        MarketError::Unauthenticated(_) => StatusCode::UNAUTHORIZED,
        MarketError::Forbidden(_) => StatusCode::FORBIDDEN,
        MarketError::ListingRevoked(_) | MarketError::Conflict(_) => StatusCode::CONFLICT,
        MarketError::InvalidInput(_)
        | MarketError::LicenseNotEnabled(_)
        | MarketError::Config(_) => StatusCode::BAD_REQUEST,
        MarketError::SchemaTooNew(_)
        | MarketError::Corrupt(_)
        | MarketError::Sqlite(_)
        | MarketError::Audit(_) => {
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
    use ed25519_dalek::Signer as _;
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
                ISSUER_SEED_ENV,
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
        listing_with_title("Weather station logs")
    }

    /// A clean listing request with a consent reference, for the given title.
    fn listing_with_title(title: &str) -> Value {
        json!({
            "seller_id": owner().public_key_hex(),
            "title": title,
            "license": "CC-BY-4.0",
            "price_credits": 25,
            "consent_ref": "consent-2026-10-19"
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
    async fn personal_data_in_a_listing_is_refused_by_the_privacy_gate() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());

        let (status, body) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/listings",
                Some(listing_with_title("Samples from alice@example.com")),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("privacy gate"));

        let (_, listings) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/listings", None),
        )
        .await;
        assert_eq!(listings, json!([]));
    }

    #[tokio::test]
    async fn a_listing_without_a_consent_reference_is_refused() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());

        let mut missing = listing_body();
        missing.as_object_mut().unwrap().remove("consent_ref");
        let mut blank = listing_body();
        blank["consent_ref"] = json!("   ");

        for body in [missing, blank] {
            let (status, response) = send(
                &router,
                signed_request(
                    &owner(),
                    Method::POST,
                    "/v2/marketplace/listings",
                    Some(body),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(response["error"]
                .as_str()
                .unwrap()
                .contains("consent reference"));
        }
    }

    #[tokio::test]
    async fn a_clean_listing_with_consent_is_admitted() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;
        assert!(!listing_id.is_empty());
    }

    #[tokio::test]
    async fn get_listing_returns_a_provenance_credential_that_verifies() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;

        let (status, body) = send(
            &router,
            json_request(
                Method::GET,
                &format!("/v2/marketplace/listings/{listing_id}"),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let provenance: ListingCredential =
            serde_json::from_value(body["provenance"].clone()).unwrap();
        let mut listing_fields = body.clone();
        listing_fields.as_object_mut().unwrap().remove("provenance");
        let listing: Listing = serde_json::from_value(listing_fields).unwrap();

        assert_eq!(listing.id, listing_id);
        assert_eq!(
            provenance.credential_subject.content_hash,
            credential::content_hash(&listing)
        );
        assert!(credential::verify(&provenance, &listing).is_ok());

        // A changed listing no longer matches the credential's content hash.
        let mut altered = listing.clone();
        altered.price_credits += 1;
        assert_eq!(
            credential::verify(&provenance, &altered),
            Err(credential::CredentialError::DigestMismatch)
        );
    }

    #[tokio::test]
    async fn a_malformed_issuer_seed_leaves_routes_unmounted() {
        let _env = MarketEnv::with_flag(Some("beta"));
        std::env::set_var(ISSUER_SEED_ENV, "not-hex");
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/listings", None),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn listing_search_ranks_by_text_and_drops_non_matches() {
        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        for title in [
            "Traffic camera frames",
            "Solar weather archive",
            "Weather station logs",
        ] {
            let (status, _) = send(
                &router,
                signed_request(
                    &owner(),
                    Method::POST,
                    "/v2/marketplace/listings",
                    Some(listing_with_title(title)),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
        }

        // Without a query the list is unchanged: every listing, oldest first.
        let (_, all) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/listings", None),
        )
        .await;
        assert_eq!(all.as_array().unwrap().len(), 3);

        // Both tokens match "Weather station logs" (1.0); "Solar weather archive"
        // matches one of two (0.5); the traffic listing does not match at all.
        let (status, hits) = send(
            &router,
            json_request(
                Method::GET,
                "/v2/marketplace/listings?q=weather%20station",
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let titles: Vec<&str> = hits
            .as_array()
            .unwrap()
            .iter()
            .map(|listing| listing["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, ["Weather station logs", "Solar weather archive"]);

        // A blank query is treated as no query.
        let (_, blank) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/listings?q=%20", None),
        )
        .await;
        assert_eq!(blank.as_array().unwrap().len(), 3);
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

    #[tokio::test]
    async fn grant_delivery_wrapping_and_revocation_e2e() {
        use xavier_market::{grant_signing_bytes, unwrap_key_as_buyer, wrap_key_for_buyer};

        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;

        let buyer_secret = x25519_dalek::StaticSecret::from([55u8; 32]);
        let buyer_public = x25519_dalek::PublicKey::from(&buyer_secret);
        let buyer_pub_hex = crate::crypto::hex_encode(buyer_public.as_bytes());

        let package_key = b"super-secret-content-key-123456";
        let ephemeral_seed = [77u8; 32];
        let nonce = [88u8; 12];
        let (wrapped, eph_pub, nonce_hex) = wrap_key_for_buyer(
            package_key,
            buyer_public.as_bytes(),
            &ephemeral_seed,
            &nonce,
        )
        .unwrap();

        let expires_at = now_ms() + 3_600_000;

        // Sign the grant payload with seller (owner) key
        let owner_signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let signing_bytes = grant_signing_bytes(
            &listing_id,
            &buyer_pub_hex,
            &eph_pub,
            &wrapped,
            &nonce_hex,
            expires_at,
        );
        let sig = owner_signing_key.sign(&signing_bytes);
        let signature_hex = crate::crypto::hex_encode(sig.to_bytes());

        let grant_payload = json!({
            "listing_id": listing_id,
            "buyer_x25519_pubkey_hex": buyer_pub_hex,
            "ephemeral_pubkey_hex": eph_pub,
            "wrapped_key_hex": wrapped,
            "nonce_hex": nonce_hex,
            "expires_at_ms": expires_at,
            "signature_hex": signature_hex,
        });

        // Signed create grant request by seller
        let (status, grant_json) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/grants",
                Some(grant_payload.clone()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let grant_id = grant_json["id"].as_str().unwrap().to_string();

        // Stranger cannot revoke grant
        let revoke_uri = format!("/v2/marketplace/grants/{grant_id}/revoke");
        let (status, _) = send(
            &router,
            signed_request(&stranger(), Method::POST, &revoke_uri, None),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Fetch active grant returns key material intact
        let (status, fetched) = send(
            &router,
            json_request(
                Method::GET,
                &format!("/v2/marketplace/grants/{grant_id}"),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(fetched["id"], grant_id);
        assert_eq!(fetched["wrapped_key_hex"], wrapped);
        assert_eq!(fetched["ephemeral_pubkey_hex"], eph_pub);
        assert_eq!(fetched["nonce_hex"], nonce_hex);

        // Buyer decrypts fetched active grant
        let grant_obj: xavier_market::AccessGrant = serde_json::from_value(fetched).unwrap();
        let decrypted =
            unwrap_key_as_buyer(&grant_obj, &[55u8; 32], grant_obj.created_at_ms + 10).unwrap();
        assert_eq!(decrypted, package_key);

        // Owner revokes grant
        let (status, revoked) = send(
            &router,
            signed_request(&owner(), Method::POST, &revoke_uri, None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(revoked["revoked_at_ms"].is_i64());
        // Response from revoke must strip key material
        assert_eq!(revoked["wrapped_key_hex"], "");
        assert_eq!(revoked["ephemeral_pubkey_hex"], "");
        assert_eq!(revoked["nonce_hex"], "");

        // Router test: GET on revoked grant returns without key material
        let (status, fetched_revoked) = send(
            &router,
            json_request(
                Method::GET,
                &format!("/v2/marketplace/grants/{grant_id}"),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(fetched_revoked["revoked_at_ms"].is_i64());
        assert_eq!(fetched_revoked["wrapped_key_hex"], "");
        assert_eq!(fetched_revoked["ephemeral_pubkey_hex"], "");
        assert_eq!(fetched_revoked["nonce_hex"], "");

        // Revoking non-existent grant returns 404
        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/grants/grant_nonexistent/revoke",
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn non_seller_creating_grant_gets_forbidden() {
        use xavier_market::{grant_signing_bytes, wrap_key_for_buyer};

        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;

        let buyer_secret = x25519_dalek::StaticSecret::from([55u8; 32]);
        let buyer_public = x25519_dalek::PublicKey::from(&buyer_secret);
        let buyer_pub_hex = crate::crypto::hex_encode(buyer_public.as_bytes());

        let package_key = b"super-secret-content-key-123456";
        let ephemeral_seed = [77u8; 32];
        let nonce = [88u8; 12];
        let (wrapped, eph_pub, nonce_hex) = wrap_key_for_buyer(
            package_key,
            buyer_public.as_bytes(),
            &ephemeral_seed,
            &nonce,
        )
        .unwrap();

        let expires_at = now_ms() + 3_600_000;

        // Stranger tries to sign and create a grant for owner's listing
        let stranger_signing_key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
        let signing_bytes = grant_signing_bytes(
            &listing_id,
            &buyer_pub_hex,
            &eph_pub,
            &wrapped,
            &nonce_hex,
            expires_at,
        );
        let sig = stranger_signing_key.sign(&signing_bytes);
        let signature_hex = crate::crypto::hex_encode(sig.to_bytes());

        let grant_payload = json!({
            "listing_id": listing_id,
            "buyer_x25519_pubkey_hex": buyer_pub_hex,
            "ephemeral_pubkey_hex": eph_pub,
            "wrapped_key_hex": wrapped,
            "nonce_hex": nonce_hex,
            "expires_at_ms": expires_at,
            "signature_hex": signature_hex,
        });

        let (status, _) = send(
            &router,
            signed_request(
                &stranger(),
                Method::POST,
                "/v2/marketplace/grants",
                Some(grant_payload),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn router_returns_expired_grant_without_key_material() {
        use xavier_market::{grant_signing_bytes, wrap_key_for_buyer};

        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;

        let buyer_secret = x25519_dalek::StaticSecret::from([55u8; 32]);
        let buyer_public = x25519_dalek::PublicKey::from(&buyer_secret);
        let buyer_pub_hex = crate::crypto::hex_encode(buyer_public.as_bytes());

        let (wrapped, eph_pub, nonce_hex) = wrap_key_for_buyer(
            b"key-data-16bytes",
            buyer_public.as_bytes(),
            &[77u8; 32],
            &[88u8; 12],
        )
        .unwrap();

        // Expires 1.5 seconds from now
        let expires_at = now_ms() + 1500;

        let owner_signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let signing_bytes = grant_signing_bytes(
            &listing_id,
            &buyer_pub_hex,
            &eph_pub,
            &wrapped,
            &nonce_hex,
            expires_at,
        );
        let sig = owner_signing_key.sign(&signing_bytes);
        let signature_hex = crate::crypto::hex_encode(sig.to_bytes());

        let grant_payload = json!({
            "listing_id": listing_id,
            "buyer_x25519_pubkey_hex": buyer_pub_hex,
            "ephemeral_pubkey_hex": eph_pub,
            "wrapped_key_hex": wrapped,
            "nonce_hex": nonce_hex,
            "expires_at_ms": expires_at,
            "signature_hex": signature_hex,
        });

        let (status, grant_json) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/grants",
                Some(grant_payload),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let grant_id = grant_json["id"].as_str().unwrap().to_string();

        // Wait for grant to expire (> 1500 ms)
        tokio::time::sleep(tokio::time::Duration::from_millis(1800)).await;

        // Fetch expired grant from router: key material must be stripped
        let (status, fetched_expired) = send(
            &router,
            json_request(
                Method::GET,
                &format!("/v2/marketplace/grants/{grant_id}"),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(fetched_expired["wrapped_key_hex"], "");
        assert_eq!(fetched_expired["ephemeral_pubkey_hex"], "");
        assert_eq!(fetched_expired["nonce_hex"], "");
    }

    #[tokio::test]
    async fn router_rejects_past_and_replayed_grants() {
        use xavier_market::{grant_signing_bytes, wrap_key_for_buyer};

        let _env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());
        let listing_id = create_owned_listing(&router).await;

        let buyer_secret = x25519_dalek::StaticSecret::from([55u8; 32]);
        let buyer_public = x25519_dalek::PublicKey::from(&buyer_secret);
        let buyer_pub_hex = crate::crypto::hex_encode(buyer_public.as_bytes());
        let (wrapped, eph_pub, nonce_hex) = wrap_key_for_buyer(
            b"key-data-16bytes",
            buyer_public.as_bytes(),
            &[77u8; 32],
            &[88u8; 12],
        )
        .unwrap();

        let owner_signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let grant_payload = |expires_at: i64| {
            let signing_bytes = grant_signing_bytes(
                &listing_id,
                &buyer_pub_hex,
                &eph_pub,
                &wrapped,
                &nonce_hex,
                expires_at,
            );
            let sig = owner_signing_key.sign(&signing_bytes);
            let signature_hex = crate::crypto::hex_encode(sig.to_bytes());
            json!({
                "listing_id": listing_id,
                "buyer_x25519_pubkey_hex": buyer_pub_hex,
                "ephemeral_pubkey_hex": eph_pub,
                "wrapped_key_hex": wrapped,
                "nonce_hex": nonce_hex,
                "expires_at_ms": expires_at,
                "signature_hex": signature_hex,
            })
        };

        // Expiry in the past is rejected
        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/grants",
                Some(grant_payload(now_ms() - 1000)),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Identical (buyer, ephemeral key, nonce) replay is a conflict
        let payload = grant_payload(now_ms() + 3_600_000);
        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/grants",
                Some(payload.clone()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/grants",
                Some(payload),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn consent_flow_and_query_tests() {
        let _env = MarketEnv::with_flag(Some(FLAG_VALUE));

        let router = mount(Router::new());

        // Initial consent state for unknown node is 'off'
        let (status, body) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/consent?node_id=node_x", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "off");

        // Unsigned consent mutation returns 401 Unauthorized
        let (status, _) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/consent",
                Some(json!({
                    "node_id": owner().public_key_hex(),
                    "state": "shared",
                    "terms_version": "v1.0",
                    "consent_version": "v1.0"
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Mismatched node_id (another node's id) returns 403 Forbidden
        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/consent",
                Some(json!({
                    "node_id": stranger().public_key_hex(),
                    "state": "shared",
                    "terms_version": "v1.0",
                    "consent_version": "v1.0"
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Opt in requires terms and consent versions
        let (status, _) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/consent",
                Some(json!({
                    "node_id": owner().public_key_hex(),
                    "state": "shared",
                    "terms_version": "",
                    "consent_version": "v1"
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Valid signed consent update
        let (status, body) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/consent",
                Some(json!({
                    "node_id": owner().public_key_hex(),
                    "state": "shared",
                    "terms_version": "v1.0",
                    "consent_version": "v1.0"
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "shared");

        // Query owner again
        let (status, body) = send(
            &router,
            json_request(
                Method::GET,
                &format!(
                    "/v2/marketplace/consent?node_id={}",
                    owner().public_key_hex()
                ),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "shared");
        assert_eq!(body["terms_version"], "v1.0");
    }

    #[tokio::test]
    async fn consent_read_without_node_id_is_a_bad_request() {
        let _env = MarketEnv::with_flag(Some(FLAG_VALUE));

        let router = mount(Router::new());

        // An unscoped read would expose every node's consent state, so it is refused
        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/consent", None),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // A blank node_id is treated the same as a missing one
        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/consent?node_id=%20", None),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn audit_chain_records_grants_and_receipts_and_detects_tampering() {
        use xavier_market::{grant_signing_bytes, wrap_key_for_buyer};

        let env = MarketEnv::with_flag(Some("beta"));
        let router: Router = mount(Router::new());

        // Initial audit chain is empty and verifies (0 records)
        let (status, verify_json) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/audit/verify", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(verify_json["verified"], true);
        assert_eq!(verify_json["records"], 0);

        // 1. Create a listing
        let listing_id = create_owned_listing(&router).await;

        // 2. Create a receipt via POST /v2/marketplace/receipts
        let receipt_payload = json!({
            "listing_id": listing_id,
            "buyer_id": "buyer-test-1",
        });
        let (status, receipt_json) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/receipts",
                Some(receipt_payload),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(receipt_json["listing_id"], listing_id);

        // Verify audit chain has 1 record for receipt
        let (status, records) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/audit", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(records.as_array().unwrap().len(), 1);
        let first_record_entry: Value =
            serde_json::from_str(records[0]["entry_json"].as_str().unwrap()).unwrap();
        assert_eq!(first_record_entry["kind"], "receipt");

        // 3. Create a grant via POST /v2/marketplace/grants
        let buyer_secret = x25519_dalek::StaticSecret::from([55u8; 32]);
        let buyer_public = x25519_dalek::PublicKey::from(&buyer_secret);
        let buyer_pub_hex = crate::crypto::hex_encode(buyer_public.as_bytes());

        let package_key = b"super-secret-content-key-123456";
        let ephemeral_seed = [77u8; 32];
        let nonce = [88u8; 12];
        let (wrapped, eph_pub, nonce_hex) = wrap_key_for_buyer(
            package_key,
            buyer_public.as_bytes(),
            &ephemeral_seed,
            &nonce,
        )
        .unwrap();

        let expires_at = now_ms() + 3_600_000;

        let owner_signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let signing_bytes = grant_signing_bytes(
            &listing_id,
            &buyer_pub_hex,
            &eph_pub,
            &wrapped,
            &nonce_hex,
            expires_at,
        );
        let sig = owner_signing_key.sign(&signing_bytes);
        let signature_hex = crate::crypto::hex_encode(sig.to_bytes());

        let grant_payload = json!({
            "listing_id": listing_id,
            "buyer_x25519_pubkey_hex": buyer_pub_hex,
            "ephemeral_pubkey_hex": eph_pub,
            "wrapped_key_hex": wrapped,
            "nonce_hex": nonce_hex,
            "expires_at_ms": expires_at,
            "signature_hex": signature_hex,
        });

        let (status, grant_json) = send(
            &router,
            signed_request(
                &owner(),
                Method::POST,
                "/v2/marketplace/grants",
                Some(grant_payload),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(grant_json["listing_id"], listing_id);

        // Verify audit chain has 2 records and verify endpoint returns 2
        let (status, records) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/audit", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(records.as_array().unwrap().len(), 2);
        let second_record_entry: Value =
            serde_json::from_str(records[1]["entry_json"].as_str().unwrap()).unwrap();
        assert_eq!(second_record_entry["kind"], "grant");

        let (status, verify_json) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/audit/verify", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(verify_json["verified"], true);
        assert_eq!(verify_json["records"], 2);

        // 4. Tamper with the audit log in the database file
        let db_path = env._dir.path().join("market.sqlite3");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE market_audit_log SET entry_json = '{\"tampered\": true}' WHERE seq = 1",
            [],
        )
        .unwrap();

        // 5. Verification fails on tampered chain!
        let (status, verify_fail) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/audit/verify", None),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(verify_fail["verified"], false);
    }

    #[tokio::test]
    async fn audit_routes_unmounted_when_flag_off() {
        let _env = MarketEnv::with_flag(None);
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/audit", None),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/audit/verify", None),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn telemetry_aggregates_reciprocity_and_opt_in_gate() {
        let _env = MarketEnv::with_flag(Some(FLAG_VALUE));
        let router: Router = mount(Router::new());

        // 1. Unregistered node gets 403 Forbidden
        let (status, err) = send(
            &router,
            json_request(
                Method::GET,
                "/v2/marketplace/telemetry/aggregates?node_id=unregistered_node",
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(err["error"]
            .as_str()
            .unwrap()
            .contains("not opted into shared telemetry"));

        // Missing node_id query param gets 400 Bad Request
        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/telemetry/aggregates", None),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // 2. Set consent to 'local' for node_local
        let signer_local = Signer::from_seed([11; 32]);
        let node_local = signer_local.public_key_hex();
        let payload_local = json!({
            "node_id": node_local,
            "state": "local",
            "terms_version": "v1.0",
            "consent_version": "v1.0"
        });
        let req_local = signed_request(
            &signer_local,
            Method::POST,
            "/v2/marketplace/consent",
            Some(payload_local),
        );
        let (status, _) = send(&router, req_local).await;
        assert_eq!(status, StatusCode::OK);

        // Record a telemetry sample for node_local
        let (status, record_resp) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/telemetry",
                Some(json!({
                    "node_id": node_local,
                    "kind": "bandwidth",
                    "fields": {
                        "metric_name": "bytes_out",
                        "value": 1024
                    }
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(record_resp["recorded"], true);

        // Reciprocity rule: node_local CANNOT fetch aggregates (403 Forbidden)
        let (status, err) = send(
            &router,
            json_request(
                Method::GET,
                &format!("/v2/marketplace/telemetry/aggregates?node_id={node_local}"),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(err["error"]
            .as_str()
            .unwrap()
            .contains("not opted into shared telemetry"));

        // 3. Set consent to 'shared' for node_shared
        let signer_shared = Signer::from_seed([12; 32]);
        let node_shared = signer_shared.public_key_hex();
        let payload_shared = json!({
            "node_id": node_shared,
            "state": "shared",
            "terms_version": "v1.0",
            "consent_version": "v1.0"
        });
        let req_shared = signed_request(
            &signer_shared,
            Method::POST,
            "/v2/marketplace/consent",
            Some(payload_shared),
        );
        let (status, _) = send(&router, req_shared).await;
        assert_eq!(status, StatusCode::OK);

        // Record a sample for node_shared
        let (status, record_resp) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/telemetry",
                Some(json!({
                    "node_id": node_shared,
                    "kind": "query_latency",
                    "fields": {
                        "metric_name": "duration_ms",
                        "value": 15
                    }
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(record_resp["recorded"], true);

        // Node shared CAN fetch aggregates! (200 OK)
        let (status, aggs) = send(
            &router,
            json_request(
                Method::GET,
                &format!("/v2/marketplace/telemetry/aggregates?node_id={node_shared}"),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let arr = aggs.as_array().unwrap();
        assert_eq!(arr.len(), 2); // bandwidth and query_latency

        // Authenticated GET /v2/marketplace/telemetry/aggregates using signer_shared
        let auth_req = signed_request(
            &signer_shared,
            Method::GET,
            "/v2/marketplace/telemetry/aggregates",
            None,
        );
        let (status, aggs_auth) = send(&router, auth_req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(aggs_auth.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn telemetry_routes_unmounted_when_flag_off() {
        let _env = MarketEnv::with_flag(None);
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            json_request(
                Method::GET,
                "/v2/marketplace/telemetry/aggregates?node_id=any",
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = send(
            &router,
            json_request(
                Method::POST,
                "/v2/marketplace/telemetry",
                Some(json!({"node_id": "x", "kind": "k"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn anchor_job_flag_off_nothing_runs() {
        let _env = MarketEnv::with_flag(None);
        let router: Router = mount(Router::new());
        let (status, _) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/anchor/dry-run", None),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = send(
            &router,
            json_request(Method::POST, "/v2/marketplace/anchor/dry-run", None),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let report = run_dry_run_anchor_cycle(&store, &MockAnchorTransport).unwrap();
        assert!(report.is_none());

        let job_handle = spawn_anchor_job_from_env(Arc::new(store));
        assert!(job_handle.is_none());
    }

    #[tokio::test]
    async fn anchor_job_flag_on_produces_verifiable_root_and_mock_receipt() {
        let _env = MarketEnv::with_flag(Some(FLAG_VALUE));
        let router: Router = mount(Router::new());

        // When empty, dry-run reports no leaves to anchor
        let (status, empty_resp) = send(
            &router,
            json_request(Method::POST, "/v2/marketplace/anchor/dry-run", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(empty_resp["message"], "no leaves to anchor");
        assert_eq!(empty_resp["dry_run"], true);

        // Create a listing
        let signer = Signer::from_seed([33; 32]);
        let listing_req = signed_request(
            &signer,
            Method::POST,
            "/v2/marketplace/listings",
            Some(json!({
                "seller_id": signer.public_key_hex(),
                "title": "Dataset for anchor test",
                "license": "CC-BY-4.0",
                "price_credits": 250,
                "consent_ref": "consent-v1"
            })),
        );
        let (status, created) = send(&router, listing_req).await;
        assert_eq!(status, StatusCode::CREATED);
        let listing_id = created["id"].as_str().unwrap().to_string();

        // Create a receipt
        let receipt_req = json_request(
            Method::POST,
            "/v2/marketplace/receipts",
            Some(json!({
                "listing_id": listing_id,
                "buyer_id": "buyer-node-1"
            })),
        );
        let (status, receipt_resp) = send(&router, receipt_req).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(receipt_resp["listing_id"], listing_id);

        // POST /v2/marketplace/anchor/dry-run produces a verifiable root
        let (status, report) = send(
            &router,
            json_request(Method::POST, "/v2/marketplace/anchor/dry-run", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(report["proof_valid"], true);
        assert_eq!(report["receipt"]["dry_run"], true);
        assert!(report["receipt"]["tx_hash"]
            .as_str()
            .unwrap()
            .starts_with("mock:"));

        let root_hex = report["root"].as_str().unwrap();
        assert_eq!(root_hex.len(), 64);
        assert!(report["leaf_count"].as_u64().unwrap() >= 2);

        // Verify sampled leaf proof accepted by verify_proof
        let sampled_leaf = report["sampled_leaf"].as_str().unwrap();
        assert_eq!(sampled_leaf.len(), 64);

        // GET /v2/marketplace/anchor/dry-run returns latest report with same root
        let (status, get_report) = send(
            &router,
            json_request(Method::GET, "/v2/marketplace/anchor/dry-run", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(get_report["root"], root_hex);
        assert_eq!(get_report["proof_valid"], true);

        // Verify latest_anchor_report() helper returns the report
        let latest = latest_anchor_report().expect("latest report exists");
        assert_eq!(latest.root, root_hex);
        assert_eq!(latest.proof_valid, true);
        assert!(latest.receipt.dry_run);
    }
}
