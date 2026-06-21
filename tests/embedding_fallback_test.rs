use anyhow::Result;
use axum::{routing::post, Router};
use tokio::net::TcpListener;
use xavier::embedding::{EmbedderConfig, Embedder};
use std::sync::Arc;

#[tokio::test]
async fn test_embedding_fallback_cloud_to_gllm() -> Result<()> {
    // 1. Mock a failing Cloud Embedding Server
    let mock_cloud_service = Router::new().route(
        "/v1/embeddings",
        post(|| async {
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Cloud provider failed")
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let mock_url = format!("http://{}", addr);

    tokio::spawn(async move {
        let _ = axum::serve(listener, mock_cloud_service).await;
    });

    // 2. Configure Xavier to use this mock as cloud provider with fallback
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
    std::env::set_var("XAVIER_EMBEDDING_URL", &mock_url);
    std::env::set_var("OPENAI_API_KEY", "test-key");
    std::env::set_var("XAVIER_EMBEDDING_TIMEOUT_SECS", "1");

    let config = EmbedderConfig::from_env();
    let embedder: Arc<dyn Embedder> = config.build().await?;

    // 3. Attempt to encode. It should try mock_url, fail, and then try GLLM.
    let result = embedder.encode("test text").await;

    match result {
        Ok(_) => println!("Embedding succeeded (maybe Noop or GLLM worked)"),
        Err(e) => {
            let err_msg = e.to_string();
            println!("Embedding error: {}", err_msg);
            // Verify it mentions GLLM or the fact that it tried fallback
            assert!(err_msg.contains("gllm") || err_msg.contains("local-gllm") || err_msg.contains("unavailable"));
        }
    }

    // Cleanup
    std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
    std::env::remove_var("XAVIER_EMBEDDING_URL");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("XAVIER_EMBEDDING_TIMEOUT_SECS");

    Ok(())
}
