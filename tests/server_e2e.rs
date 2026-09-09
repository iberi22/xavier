use std::net::TcpListener;
use std::process::{Child, Stdio};
use std::time::Duration;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_health_endpoint_via_xavier_binary() {
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();
    let url = format!("http://127.0.0.1:{port}");
    let _child = ChildGuard(
        std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
            .arg("http")
            .arg(port.to_string())
            .arg("--mcp-port")
            .arg("0")
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_URL", &url)
            .env("XAVIER_TOKEN", "test-token")
            .env("XAVIER_MCP_PORT", "0")
            .env(
                "XAVIER_CODE_GRAPH_DB_PATH",
                data_dir.path().join(format!("e2e-code-graph-{port}.db")),
            )
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                data_dir.path().join(format!("e2e-memory-vec-{port}.db")),
            )
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start xavier binary"),
    );

    let client = reqwest::Client::new();
    let health_url = format!("{url}/health");
    let readiness_url = format!("{url}/readiness");
    let protected_url = format!("{url}/v1/account/usage");
    let mut healthy = false;
    let mut readiness_checked = false;
    let mut auth_checked = false;

    for _ in 0..120 {
        match client.get(&health_url).send().await {
            Ok(response) if response.status().is_success() => {
                assert!(response.headers().contains_key("x-request-id"));
                let body = response.text().await.expect("health body");
                assert!(
                    body.contains("\"status\":\"healthy\"")
                        || body.contains("\"status\":\"degraded\"")
                        || body.contains("\"status\":\"unhealthy\"")
                        || body.contains("\"status\":\"ok\"")
                );
                healthy = true;

                let readiness = client
                    .get(&readiness_url)
                    .send()
                    .await
                    .expect("readiness response");
                assert!(readiness.status().is_success());
                assert!(readiness.headers().contains_key("x-request-id"));
                let readiness_body = readiness.text().await.expect("readiness body");
                assert!(readiness_body.contains("\"service\":\"xavier\""));
                assert!(
                    readiness_body.contains("\"status\":\"ok\"")
                        || readiness_body.contains("\"status\":\"degraded\"")
                );
                readiness_checked = true;

                let protected = client
                    .get(&protected_url)
                    .send()
                    .await
                    .expect("protected response");
                assert_eq!(protected.status(), reqwest::StatusCode::UNAUTHORIZED);
                assert!(protected.headers().contains_key("x-request-id"));

                let authorized = client
                    .get(&protected_url)
                    .header("X-Xavier-Token", "test-token")
                    .send()
                    .await
                    .expect("authorized response");
                assert!(authorized.status().is_success());
                assert!(authorized.headers().contains_key("x-request-id"));
                let usage: serde_json::Value = authorized.json().await.expect("usage json");
                assert!(usage.get("optimization").is_some());
                assert!(usage["optimization"].get("router_direct_count").is_some());
                assert!(usage["optimization"].get("semantic_cache_hits").is_some());

                // E2E Auth test for GET /v1/memories/{id}/outline
                let outline_unauth = client
                    .get(&format!("{url}/v1/memories/test-mem-id/outline"))
                    .send()
                    .await
                    .expect("outline unauth response");
                assert_eq!(outline_unauth.status(), reqwest::StatusCode::UNAUTHORIZED);

                let outline_auth = client
                    .get(&format!("{url}/v1/memories/non-existent-id/outline"))
                    .header("X-Xavier-Token", "test-token")
                    .send()
                    .await
                    .expect("outline auth response");
                assert!(outline_auth.status().is_success());
                let outline_json: serde_json::Value = outline_auth.json().await.expect("outline json");
                assert_eq!(outline_json["status"], "error");
                assert_eq!(outline_json["code"], "NOT_FOUND");

                // E2E Auth test for POST /v1/memories/prune
                let prune_unauth = client
                    .post(&format!("{url}/v1/memories/prune"))
                    .header("Content-Type", "application/json")
                    .body(r#"{"older_than_days": 30, "dry_run": true}"#)
                    .send()
                    .await
                    .expect("prune unauth response");
                assert_eq!(prune_unauth.status(), reqwest::StatusCode::UNAUTHORIZED);

                let prune_auth = client
                    .post(&format!("{url}/v1/memories/prune"))
                    .header("X-Xavier-Token", "test-token")
                    .header("Content-Type", "application/json")
                    .body(r#"{"older_than_days": 30, "dry_run": true}"#)
                    .send()
                    .await
                    .expect("prune auth response");
                assert!(prune_auth.status().is_success());

                auth_checked = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }

    assert!(healthy, "xavier did not expose a healthy /health endpoint");
    assert!(
        readiness_checked,
        "xavier did not expose a valid /readiness endpoint"
    );
    assert!(
        auth_checked,
        "xavier did not enforce authentication on protected routes"
    );
}
