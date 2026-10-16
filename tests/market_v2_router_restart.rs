//! MKT-02 acceptance: a marketplace v2 listing and receipt survive a daemon
//! restart, exercised on the production router with `market-v2` on.
//!
//! A fresh router built over the same SQLite file stands in for a restarted
//! daemon. This file is its own test binary, so the env vars it sets cannot
//! leak into the rest of the suite.

use std::sync::{Mutex, MutexGuard};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::util::ServiceExt;
use xavier::adapters::inbound::http::routes::create_router;

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Points the market store at a tempdir file and turns the beta flag on.
struct BetaEnv {
    _lock: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}

impl BetaEnv {
    fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Keep every user-state path (data dir, state, home) inside a sandbox.
        xavier::test_support::isolate_and_redirect();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(
            "XAVIER_MARKET_V2_DB_PATH",
            dir.path().join("market.sqlite3"),
        );
        std::env::set_var("XAVIER_MARKET_V2", "beta");
        Self {
            _lock: lock,
            _dir: dir,
        }
    }
}

impl Drop for BetaEnv {
    fn drop(&mut self) {
        std::env::remove_var("XAVIER_MARKET_V2");
        std::env::remove_var("XAVIER_MARKET_V2_DB_PATH");
    }
}

async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("build request");
    let response = router.clone().oneshot(request).await.expect("route");
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
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
async fn listing_and_receipt_survive_a_daemon_restart() {
    let _env = BetaEnv::new();

    let before = create_router();
    let (status, listing) = send(
        &before,
        Method::POST,
        "/v2/marketplace/listings",
        Some(listing_body()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let listing_id = listing["id"].as_str().unwrap().to_string();

    let (status, receipt) = send(
        &before,
        Method::POST,
        "/v2/marketplace/receipts",
        Some(json!({"listing_id": listing_id, "buyer_id": "buyer-1"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    drop(before);

    let after = create_router();
    let (status, fetched) = send(
        &after,
        Method::GET,
        &format!("/v2/marketplace/listings/{listing_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched, listing);

    let (status, receipts) = send(
        &after,
        Method::GET,
        "/v2/marketplace/receipts?buyer_id=buyer-1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipts, json!([receipt]));
}
