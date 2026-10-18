use axum::{extract::Extension, http::StatusCode, response::IntoResponse, Json};

use crate::adapters::inbound::http::routes::RootCredential;
use crate::codebase::connection_manager::ConnectionManager;
use crate::memory::repo_store_registry::repo_stores;

pub async fn admin_stores_handler(root: Option<Extension<RootCredential>>) -> impl IntoResponse {
    if root.is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "forbidden"})),
        )
            .into_response();
    }

    let snap = ConnectionManager::global().snapshot();
    let stats = repo_stores().stats();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "pools": snap.pools,
            "pool_counters": snap.counters,
            "repo_stores": stats
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn admin_stores_requires_root_credential() {
        let response = admin_stores_handler(None).await.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_stores_reports_open_pools() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("admin_test.db");

        ConnectionManager::global()
            .connect_with_path("admin_test_pool", db_path)
            .unwrap();

        let response = admin_stores_handler(Some(Extension(RootCredential)))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["pools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "admin_test_pool"));

        // Disconnect to clean up the global manager
        ConnectionManager::global().disconnect("admin_test_pool");
    }

    #[tokio::test]
    async fn admin_stores_reports_repo_stats_shape() {
        let response = admin_stores_handler(Some(Extension(RootCredential)))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("repo_stores").is_some());
    }
}
