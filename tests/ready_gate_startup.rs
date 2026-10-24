//! The listener serves while init is still running: /health and 503s answer at once,
//! and the application router takes over when the gate opens. Real TCP, no config or data.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use xavier::server::http::ready_gate::{serve_gated, ReadyGate};

const ONE_SECOND: Duration = Duration::from_secs(1);

struct Serving {
    addr: SocketAddr,
    gate: ReadyGate,
    stop: oneshot::Sender<()>,
    task: JoinHandle<std::io::Result<()>>,
}

/// Binds and serves through a closed gate, as the daemon does right after its bind.
async fn serve_while_init_runs() -> Serving {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let gate = ReadyGate::new();
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(serve_gated(listener, gate.clone(), async move {
        let _ = stopped.await;
    }));
    Serving {
        addr,
        gate,
        stop,
        task,
    }
}

/// The application router that init installs once it has finished.
fn initialized_app() -> Router {
    Router::new()
        .route(
            "/ready",
            get(|| async { Json(json!({"status": "ok", "service": "xavier"})) }),
        )
        .route("/v1/protected", get(|| async { "protected" }))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(ONE_SECOND)
        .build()
        .expect("client")
}

#[tokio::test]
async fn health_answers_within_one_second_while_init_is_slow() {
    let serving = serve_while_init_runs().await;
    let gate = serving.gate.clone();
    // Stands in for the slow init steps: the gate opens two seconds after the bind.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        gate.open(initialized_app());
    });

    let started = Instant::now();
    let response = client()
        .get(format!("http://{}/health", serving.addr))
        .send()
        .await
        .expect("/health must answer while starting");
    assert!(
        started.elapsed() < ONE_SECOND,
        "/health took {:?}",
        started.elapsed()
    );
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body, json!({"status": "starting"}));

    let _ = serving.stop.send(());
    serving.task.await.expect("join").expect("serve");
}

#[tokio::test]
async fn protected_route_answers_503_without_hanging_while_starting() {
    let serving = serve_while_init_runs().await;

    let response = client()
        .get(format!("http://{}/v1/protected", serving.addr))
        .send()
        .await
        .expect("protected route must not hang while starting");
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "5");
    let body: Value = response.json().await.expect("json");
    assert_eq!(body, json!({"status": "starting"}));

    let _ = serving.stop.send(());
    serving.task.await.expect("join").expect("serve");
}

#[tokio::test]
async fn ready_turns_200_once_the_gate_opens() {
    let serving = serve_while_init_runs().await;
    let url = format!("http://{}/ready", serving.addr);

    let response = client()
        .get(&url)
        .send()
        .await
        .expect("/ready while starting");
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body, json!({"status": "starting"}));

    assert!(serving.gate.open(initialized_app()));

    let response = client().get(&url).send().await.expect("/ready after open");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["status"], "ok");

    let response = client()
        .get(format!("http://{}/v1/protected", serving.addr))
        .send()
        .await
        .expect("protected route after open");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let _ = serving.stop.send(());
    serving.task.await.expect("join").expect("serve");
}

#[tokio::test]
async fn graceful_shutdown_ends_serving_while_still_starting() {
    let serving = serve_while_init_runs().await;
    let _ = serving.stop.send(());
    let result = tokio::time::timeout(Duration::from_secs(5), serving.task)
        .await
        .expect("serve must end after the shutdown signal");
    result.expect("join").expect("serve");
}
