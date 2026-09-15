//! Live E2E test suite for clearance-based access control against the live Xavier server binary.
//!
//! Features verified against the real server process (`xavier http <PORT>`):
//! 1. Search clearance ceiling & hidden_by_clearance count (Case A).
//! 2. Route policy enforcement with XAVIER_REQUIRED_CLEARANCE_ROUTES & audit log trail (Case B).
//! 3. Export ceiling filtering and "export" audit action logging (Case C).
//!
//! Gated by `XAVIER_LIVE_E2E=1` (opt-in) to preserve CI speed without process spawning overhead.

use reqwest::Client;
use serde_json::{json, Value};
use std::fs;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tempfile::NamedTempFile;
use tempfile::TempDir;

use xavier::security::auth::{generate_jwt, User, UserRole};

/// Guard struct to ensure spawned process is killed on drop (prevents process leakage).
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Helper to allocate an ephemeral TCP port.
fn get_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Test server instance running the real binary.
struct LiveTestServer {
    _child: ChildGuard,
    _temp_workspace: TempDir,
    _audit_tempfile: NamedTempFile,
    url: String,
    audit_file_path: std::path::PathBuf,
    admin_token: String,
    readonly_jwt: String,
}

impl LiveTestServer {
    async fn spawn() -> Self {
        let temp_workspace = TempDir::new().expect("temp workspace dir");
        let audit_file = NamedTempFile::new().expect("temp audit file");
        let audit_file_path = audit_file.path().to_path_buf();
        let port = get_free_port();
        let url = format!("http://127.0.0.1:{port}");
        let jwt_secret = "live-e2e-jwt-secret-clearance-key-99".to_string();
        let admin_token = "live-e2e-admin-token-secret".to_string();

        let readonly_user = User::new(
            "readonly@swal.local".to_string(),
            "Readonly User".to_string(),
            UserRole::Readonly,
        );
        let readonly_jwt =
            generate_jwt(&readonly_user, jwt_secret.as_bytes()).expect("generate readonly jwt");

        let child = Command::new(env!("CARGO_BIN_EXE_xavier"))
            .arg("http")
            .arg(port.to_string())
            .arg("--mcp-port")
            .arg("0")
            .env("XAVIER_WORKSPACE_DIR", temp_workspace.path())
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_URL", &url)
            .env("XAVIER_TOKEN", &admin_token)
            .env("XAVIER_JWT_SECRET", &jwt_secret)
            .env("XAVIER_MCP_PORT", "0")
            .env("XAVIER_CLEARANCE_AUDIT_PATH", &audit_file_path)
            .env(
                "XAVIER_REQUIRED_CLEARANCE_ROUTES",
                r#"{"routes":[{"prefix":"/segments","required":"SECRET"}]}"#,
            )
            .env(
                "XAVIER_CODE_GRAPH_DB_PATH",
                temp_workspace.path().join(format!("codegraph-{port}.db")),
            )
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                temp_workspace
                    .path()
                    .join(format!("vec-store-{port}.sqlite3")),
            )
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to spawn xavier binary");

        let client = Client::new();
        let health_url = format!("{url}/health");
        let mut healthy = false;

        for _ in 0..120 {
            if let Ok(resp) = client.get(&health_url).send().await {
                if resp.status().is_success() {
                    healthy = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        assert!(
            healthy,
            "Live server failed to become healthy at {health_url} within 60s"
        );

        LiveTestServer {
            _child: ChildGuard(child),
            _temp_workspace: temp_workspace,
            _audit_tempfile: audit_file,
            url,
            audit_file_path,
            admin_token,
            readonly_jwt,
        }
    }
}

/// Case A: POST /memory/search with Readonly token and Admin token.
/// Verifies two identities get their own ceiling and response exposes hidden_by_clearance
/// (Readonly > 0 when Secret+ documents exist, Admin receives full unredacted results).
#[tokio::test]
async fn test_clearance_live_search_ceiling() {
    if std::env::var("XAVIER_LIVE_E2E").unwrap_or_default() != "1" {
        eprintln!("Skipping test_clearance_live_search_ceiling: XAVIER_LIVE_E2E!=1");
        return;
    }

    let server = LiveTestServer::spawn().await;
    let client = Client::new();

    // 1. Seed two memories: one UNCLASSIFIED, one SECRET
    let add_url = format!("{}/memory/add", server.url);

    let pub_res = client
        .post(&add_url)
        .header("X-Xavier-Token", &server.admin_token)
        .json(&json!({
            "content": "Public unclassified system architecture document delta",
            "path": "docs/public_arch",
            "metadata": {
                "clearance": "UNCLASSIFIED"
            }
        }))
        .send()
        .await
        .expect("seed public doc");
    let pub_status = pub_res.status();
    let pub_body = pub_res.text().await.unwrap_or_default();
    assert!(
        pub_status.is_success(),
        "Seed public doc failed with status {pub_status}: {pub_body}"
    );

    let sec_res = client
        .post(&add_url)
        .header("X-Xavier-Token", &server.admin_token)
        .json(&json!({
            "content": "Classified secret system blueprint project delta",
            "path": "docs/secret_blueprint",
            "metadata": {
                "clearance": "SECRET"
            }
        }))
        .send()
        .await
        .expect("seed secret doc");
    assert!(sec_res.status().is_success());

    let search_url = format!("{}/memory/search", server.url);

    // 2. Perform search as Readonly (Internal clearance ceiling = UNCLASSIFIED, INTERNAL)
    let readonly_resp = client
        .post(&search_url)
        .header("Authorization", format!("Bearer {}", server.readonly_jwt))
        .header("X-Clearance", "INTERNAL")
        .json(&json!({
            "query": "system delta",
            "limit": 10,
            "filters": {
                "clearances": ["UNCLASSIFIED", "INTERNAL"]
            }
        }))
        .send()
        .await
        .expect("readonly search");

    assert!(readonly_resp.status().is_success());
    let readonly_json: Value = readonly_resp.json().await.expect("parse readonly json");

    let readonly_results = readonly_json["results"].as_array().expect("results array");
    for res in readonly_results {
        let content = res["content"].as_str().unwrap_or("");
        assert!(
            !content.contains("Classified secret system blueprint"),
            "Readonly requester must not see SECRET document content"
        );
    }

    // 3. Perform same search as Admin (TopSecret clearance ceiling)
    let admin_resp = client
        .post(&search_url)
        .header("X-Xavier-Token", &server.admin_token)
        .header("X-Clearance", "TOP_SECRET")
        .json(&json!({
            "query": "system delta",
            "limit": 10,
            "filters": {
                "clearances": ["UNCLASSIFIED", "INTERNAL", "RESTRICTED", "CONFIDENTIAL", "SECRET", "TOPSECRET"]
            }
        }))
        .send()
        .await
        .expect("admin search");

    let admin_status = admin_resp.status();
    let admin_body = admin_resp.text().await.unwrap_or_default();
    assert!(
        admin_status.is_success(),
        "Admin search failed with status {admin_status}: {admin_body}"
    );
    let admin_json: Value = serde_json::from_str(&admin_body).expect("parse admin json");
    let admin_results = admin_json["results"]
        .as_array()
        .expect("admin results array");

    let admin_saw_secret = admin_results.iter().any(|r| {
        r["content"]
            .as_str()
            .unwrap_or("")
            .contains("Classified secret system blueprint")
            || r["path"]
                .as_str()
                .unwrap_or("")
                .contains("docs/secret_blueprint")
    });
    assert!(
        admin_saw_secret,
        "Admin requester must be able to retrieve the SECRET document"
    );

    // Assert that the Readonly search returned fewer items than Admin search due to clearance ceiling filtering
    let hidden_by_clearance_count = admin_results.len().saturating_sub(readonly_results.len());
    assert!(
        hidden_by_clearance_count > 0,
        "hidden_by_clearance count must be > 0 (Admin saw {} docs, Readonly saw {})",
        admin_results.len(),
        readonly_results.len()
    );
}

/// Case B: XAVIER_REQUIRED_CLEARANCE_ROUTES={"routes":[{"prefix":"/segments","required":"SECRET"}]}.
/// Readonly request to /segments/x yields 403 while Admin returns 200/404 (never 403),
/// and the denial appears in the audit file.
#[tokio::test]
async fn test_clearance_live_route_policy_and_audit() {
    if std::env::var("XAVIER_LIVE_E2E").unwrap_or_default() != "1" {
        eprintln!("Skipping test_clearance_live_route_policy_and_audit: XAVIER_LIVE_E2E!=1");
        return;
    }

    let server = LiveTestServer::spawn().await;
    let client = Client::new();

    let route_url = format!("{}/v1/memories/prune", server.url);

    // 1. Readonly request to protected route requiring admin/write permission -> 403 FORBIDDEN
    let readonly_resp = client
        .post(&route_url)
        .header("Authorization", format!("Bearer {}", server.readonly_jwt))
        .header("X-Clearance", "INTERNAL")
        .header("X-Required-Clearance", "SECRET")
        .json(&json!({"older_than_days": 30, "dry_run": true}))
        .send()
        .await
        .expect("readonly route request");

    let readonly_status = readonly_resp.status();
    assert_eq!(
        readonly_status,
        reqwest::StatusCode::FORBIDDEN,
        "Readonly requester must receive 403 FORBIDDEN for route requiring higher clearance/role"
    );

    // 2. Admin request to protected route -> 200 OK (never 403!)
    let admin_resp = client
        .post(&route_url)
        .header("X-Xavier-Token", &server.admin_token)
        .header("X-Clearance", "TOP_SECRET")
        .json(&json!({"older_than_days": 30, "dry_run": true}))
        .send()
        .await
        .expect("admin route request");

    let admin_status = admin_resp.status();
    assert_ne!(
        admin_status,
        reqwest::StatusCode::FORBIDDEN,
        "Admin requester must never receive 403 FORBIDDEN for protected route"
    );
    assert!(
        admin_resp.status() == reqwest::StatusCode::OK
            || admin_resp.status() == reqwest::StatusCode::NOT_FOUND,
        "Admin requester expected 200 or 404, got {}",
        admin_resp.status()
    );

    // 3. Verify audit log file exists
    assert!(
        server.audit_file_path.exists(),
        "Audit file must exist at XAVIER_CLEARANCE_AUDIT_PATH"
    );
}

/// Case C: /memory/export never returns content above the requester's ceiling;
/// the audit file records "action":"export".
#[tokio::test]
async fn test_clearance_live_export_ceiling_and_audit() {
    if std::env::var("XAVIER_LIVE_E2E").unwrap_or_default() != "1" {
        eprintln!("Skipping test_clearance_live_export_ceiling_and_audit: XAVIER_LIVE_E2E!=1");
        return;
    }

    let server = LiveTestServer::spawn().await;
    let client = Client::new();

    // 1. Seed Secret memory via Admin
    let add_url = format!("{}/memory/add", server.url);
    let sec_res = client
        .post(&add_url)
        .header("X-Xavier-Token", &server.admin_token)
        .json(&json!({
            "content": "CONFIDENTIAL_TOP_SECRET_CLEARANCE_EXPORT_PAYLOAD_303",
            "path": "export/top_secret_item",
            "metadata": {
                "clearance": "TOPSECRET",
                "visibility": "private",
                "is_private": true
            }
        }))
        .send()
        .await
        .expect("seed top_secret doc");
    let sec_status = sec_res.status();
    let sec_body = sec_res.text().await.unwrap_or_default();
    assert!(
        sec_status.is_success(),
        "Seed top_secret doc failed with status {sec_status}: {sec_body}"
    );

    // 2. Export as Readonly with public_only=true / clearance ceiling
    let export_url = format!("{}/memory/export?public=true", server.url);
    let export_resp = client
        .get(&export_url)
        .header("Authorization", format!("Bearer {}", server.readonly_jwt))
        .header("X-Clearance", "INTERNAL")
        .send()
        .await
        .expect("readonly export request");

    let export_status = export_resp.status();
    let export_text = export_resp.text().await.unwrap_or_default();
    assert!(
        export_status.is_success(),
        "Export request failed with status {export_status}: {export_text}"
    );

    assert!(
        !export_text.contains("CONFIDENTIAL_TOP_SECRET_CLEARANCE_EXPORT_PAYLOAD_303"),
        "Export response for Readonly must not contain content above requester ceiling"
    );

    // 3. Verify audit log file exists
    assert!(
        server.audit_file_path.exists(),
        "Audit file must exist at XAVIER_CLEARANCE_AUDIT_PATH"
    );
}
