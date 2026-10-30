#![allow(unused_imports)]
#[path = "bounded_cli_support/mod.rs"]
mod bounded_cli_support;
use bounded_cli_support::Sandbox;
use mockito::Server;
use std::time::Duration;

#[tokio::test]
async fn test_space_create_success() {
    xavier::isolate_test_process!();
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/api/v1/espacio/admin/spaces")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "id": "demo",
            "description": "a b",
            "owner_node": "CLI",
            "is_public": false
        })))
        .with_status(200)
        .with_body("ok")
        .create_async()
        .await;

    let sandbox = Sandbox::new();
    let run = sandbox.run(
        sandbox
            .command(&server.url())
            .arg("space")
            .arg("create")
            .arg("demo")
            .arg("--label")
            .arg("a")
            .arg("--label")
            .arg("b")
            .env("XAVIER_TOKEN", "fake_token"),
        Duration::from_secs(10),
    );

    mock.assert_async().await;
    assert!(run.status.success(), "create should succeed");
    assert_eq!(run.stdout.trim(), "Created space demo");
}

#[tokio::test]
async fn test_space_create_conflict() {
    xavier::isolate_test_process!();
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/api/v1/espacio/admin/spaces")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "id": "demo",
            "description": "a b",
            "owner_node": "CLI",
            "is_public": false
        })))
        .with_status(409)
        .with_body("Space demo already exists")
        .create_async()
        .await;

    let sandbox = Sandbox::new();
    let run = sandbox.run(
        sandbox
            .command(&server.url())
            .arg("space")
            .arg("create")
            .arg("demo")
            .arg("--label")
            .arg("a")
            .arg("--label")
            .arg("b")
            .env("XAVIER_TOKEN", "fake_token"),
        Duration::from_secs(10),
    );

    mock.assert_async().await;
    assert!(!run.status.success(), "create should fail on conflict");
    assert!(run
        .stderr
        .contains("Failed to create space demo: Space demo already exists"));
}

#[tokio::test]
async fn test_space_list_json_success() {
    xavier::isolate_test_process!();
    let mut server = Server::new_async().await;
    let json_body = serde_json::json!([{"id": "demo"}]);
    let mock = server
        .mock("GET", "/api/v1/espacio/spaces")
        .with_status(200)
        .with_body(serde_json::to_string(&json_body).unwrap())
        .create_async()
        .await;

    let sandbox = Sandbox::new();
    let run = sandbox.run(
        sandbox
            .command(&server.url())
            .arg("space")
            .arg("list")
            .arg("--json")
            .env("XAVIER_TOKEN", "fake_token"),
        Duration::from_secs(10),
    );

    mock.assert_async().await;
    assert!(run.status.success(), "list should succeed");
    assert_eq!(run.stdout.trim(), r#"[{"id":"demo"}]"#);
}

#[tokio::test]
async fn test_space_list_json_invalid() {
    xavier::isolate_test_process!();
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/v1/espacio/spaces")
        .with_status(200)
        .with_body("not a json")
        .create_async()
        .await;

    let sandbox = Sandbox::new();
    let run = sandbox.run(
        sandbox
            .command(&server.url())
            .arg("space")
            .arg("list")
            .arg("--json")
            .env("XAVIER_TOKEN", "fake_token"),
        Duration::from_secs(10),
    );

    mock.assert_async().await;
    assert!(!run.status.success(), "list should fail with invalid JSON");
    assert!(run
        .stderr
        .contains("Failed to parse response as JSON: not a json"));
}
