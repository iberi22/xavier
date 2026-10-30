use axum::{routing::get, Json, Router};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::process::Command;
use xavier::enterprise::tenant::Plan;
use xavier::enterprise::tenant::TenantId;
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TenantResponse {
    pub id: TenantId,
    pub name: String,
    pub plan: Plan,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

xavier::isolate_test_process!();

async fn mock_server() -> SocketAddr {
    let app = Router::new()
        .route(
            "/v1/tenants",
            get(|| async {
                let tenants = vec![
                    TenantResponse {
                        id: uuid::Uuid::nil(),
                        name: "tenant-a".to_string(),
                        plan: Plan::Free,
                        created_at: chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                            .unwrap()
                            .with_timezone(&chrono::Utc),
                    },
                    TenantResponse {
                        id: uuid::Uuid::new_v4(),
                        name: "tenant-b".to_string(),
                        plan: Plan::Pro,
                        created_at: chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                            .unwrap()
                            .with_timezone(&chrono::Utc),
                    },
                ];
                Json(tenants)
            }),
        )
        .route(
            "/v1/tenants/{id}",
            get(
                |axum::extract::Path(id): axum::extract::Path<uuid::Uuid>| async move {
                    if id == uuid::Uuid::nil() {
                        let tenant = TenantResponse {
                            id: uuid::Uuid::nil(),
                            name: "tenant-a".to_string(),
                            plan: Plan::Free,
                            created_at: chrono::DateTime::parse_from_rfc3339(
                                "2024-01-01T00:00:00Z",
                            )
                            .unwrap()
                            .with_timezone(&chrono::Utc),
                        };
                        Ok(Json(tenant))
                    } else {
                        Err(axum::http::StatusCode::NOT_FOUND)
                    }
                },
            ),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

#[tokio::test]
async fn test_cli_tenants_list_text() {
    let addr = mock_server().await;
    let url = format!("http://{}", addr);

    let temp_dir = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .args(&["tenants", "list"])
        .env("XAVIER_URL", url)
        .env("XAVIER_TOKEN", "fake-token")
        .env("XAVIER_DATA_DIR", temp_dir.path())
        .env("XAVIER_CONFIG_PATH", temp_dir.path().join("config.json"))
        .env(
            "XAVIER_RUNTIME_STATE_PATH",
            temp_dir.path().join("runtime.json"),
        )
        .env("HOME", temp_dir.path())
        .output()
        .await
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("tenant-a"));
    assert!(stdout.contains("tenant-b"));
    assert!(stdout.contains("Free"));
    assert!(stdout.contains("Pro"));
}

#[tokio::test]
async fn test_cli_tenants_list_json() {
    let addr = mock_server().await;
    let url = format!("http://{}", addr);

    let temp_dir = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .args(&["tenants", "list", "--json"])
        .env("XAVIER_URL", url)
        .env("XAVIER_TOKEN", "fake-token")
        .env("XAVIER_DATA_DIR", temp_dir.path())
        .env("XAVIER_CONFIG_PATH", temp_dir.path().join("config.json"))
        .env(
            "XAVIER_RUNTIME_STATE_PATH",
            temp_dir.path().join("runtime.json"),
        )
        .env("HOME", temp_dir.path())
        .output()
        .await
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.is_array());
    let array = parsed.as_array().unwrap();
    assert_eq!(array.len(), 2);
    assert_eq!(array[0]["name"], "tenant-a");
    assert_eq!(array[0]["created_at"], "2024-01-01T00:00:00Z");
}

#[tokio::test]
async fn test_cli_tenants_show_text() {
    let addr = mock_server().await;
    let url = format!("http://{}", addr);

    let temp_dir = tempfile::tempdir().unwrap();

    let id = uuid::Uuid::nil().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .args(&["tenants", "show", &id])
        .env("XAVIER_URL", url)
        .env("XAVIER_TOKEN", "fake-token")
        .env("XAVIER_DATA_DIR", temp_dir.path())
        .env("XAVIER_CONFIG_PATH", temp_dir.path().join("config.json"))
        .env(
            "XAVIER_RUNTIME_STATE_PATH",
            temp_dir.path().join("runtime.json"),
        )
        .env("HOME", temp_dir.path())
        .output()
        .await
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("tenant-a"));
    assert!(stdout.contains(&id));
}

#[tokio::test]
async fn test_cli_tenants_show_json() {
    let addr = mock_server().await;
    let url = format!("http://{}", addr);

    let temp_dir = tempfile::tempdir().unwrap();

    let id = uuid::Uuid::nil().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .args(&["tenants", "show", &id, "--json"])
        .env("XAVIER_URL", url)
        .env("XAVIER_TOKEN", "fake-token")
        .env("XAVIER_DATA_DIR", temp_dir.path())
        .env("XAVIER_CONFIG_PATH", temp_dir.path().join("config.json"))
        .env(
            "XAVIER_RUNTIME_STATE_PATH",
            temp_dir.path().join("runtime.json"),
        )
        .env("HOME", temp_dir.path())
        .output()
        .await
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.is_object());
    assert_eq!(parsed["name"], "tenant-a");
    assert_eq!(parsed["id"], id);
    assert_eq!(parsed["created_at"], "2024-01-01T00:00:00Z");
}

#[tokio::test]
async fn test_cli_tenants_show_not_found() {
    let addr = mock_server().await;
    let url = format!("http://{}", addr);

    let temp_dir = tempfile::tempdir().unwrap();

    let fake_id = uuid::Uuid::new_v4().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .args(&["tenants", "show", &fake_id])
        .env("XAVIER_URL", url)
        .env("XAVIER_TOKEN", "fake-token")
        .env("XAVIER_DATA_DIR", temp_dir.path())
        .env("XAVIER_CONFIG_PATH", temp_dir.path().join("config.json"))
        .env(
            "XAVIER_RUNTIME_STATE_PATH",
            temp_dir.path().join("runtime.json"),
        )
        .env("HOME", temp_dir.path())
        .output()
        .await
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("tenant not found"));
}

#[tokio::test]
async fn test_cli_tenants_unauthorized_exit_code() {
    let app = Router::new().route(
        "/v1/tenants",
        get(|| async { Err::<(), _>(axum::http::StatusCode::UNAUTHORIZED) }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}", addr);

    let temp_dir = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .args(&["tenants", "list"])
        .env("XAVIER_URL", url)
        .env("XAVIER_TOKEN", "fake-token")
        .env("XAVIER_DATA_DIR", temp_dir.path())
        .env("XAVIER_CONFIG_PATH", temp_dir.path().join("config.json"))
        .env(
            "XAVIER_RUNTIME_STATE_PATH",
            temp_dir.path().join("runtime.json"),
        )
        .env("HOME", temp_dir.path())
        .output()
        .await
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3)); // 3 is the standard auth exit code in this repo
}
