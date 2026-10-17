//! MKT-17 IVN routes over the marketplace v2 store (`/v2/ivn/*`).
//!
//! Thin on purpose: validation, signature checks and state rules live in
//! `xavier_market`. Validators are not registered over HTTP. A request token
//! alone must not be able to add a validator and then vote as it, so the
//! registry is filled only by code that holds the store. An operator-only path
//! is a follow-up.

use std::sync::Arc;

use axum::{
    extract::Path as PathParam,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use xavier_market::{CastVote, MarketStore, NewIdentityRequest, VoteDecision};

use super::{error_response, error_status};

/// The `/v2/ivn/*` routes. They read the store from the `Extension` that the
/// parent router attaches, so they must be merged before that layer.
pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/v2/ivn/validators", get(list_validators_handler))
        .route("/v2/ivn/requests", post(create_request_handler))
        .route("/v2/ivn/requests/{id}", get(get_request_handler))
        .route("/v2/ivn/requests/{id}/votes", post(cast_vote_handler))
}

type StoreExt = Extension<Arc<MarketStore>>;

async fn list_validators_handler(Extension(store): StoreExt) -> Response {
    match store.list_validators() {
        Ok(validators) => Json(validators).into_response(),
        Err(err) => error_response(err),
    }
}

async fn create_request_handler(
    Extension(store): StoreExt,
    Json(new): Json<NewIdentityRequest>,
) -> Response {
    match store.create_identity_request(new) {
        Ok(request) => (StatusCode::CREATED, Json(request)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn get_request_handler(
    Extension(store): StoreExt,
    PathParam(id): PathParam<String>,
) -> Response {
    match store.get_identity_request(&id) {
        Ok(Some(request)) => Json(request).into_response(),
        Ok(None) => error_status(
            StatusCode::NOT_FOUND,
            format!("identity request {id} not found"),
        ),
        Err(err) => error_response(err),
    }
}

/// Body of a vote. The request id comes from the path, never from the body.
#[derive(Debug, Deserialize)]
struct VoteBody {
    validator_id: String,
    decision: VoteDecision,
    signature: String,
}

async fn cast_vote_handler(
    Extension(store): StoreExt,
    PathParam(id): PathParam<String>,
    Json(body): Json<VoteBody>,
) -> Response {
    let cast = CastVote {
        request_id: id,
        validator_id: body.validator_id,
        decision: body.decision,
        signature: body.signature,
    };
    match store.cast_vote(cast) {
        Ok(request) => (StatusCode::CREATED, Json(request)).into_response(),
        Err(err) => error_response(err),
    }
}
