use xavier::embedding::selector::{ContentModality, EmbeddingSelector};
use xavier::embedding::Embedder;
use xavier::server::alerts::SYSTEM_ALERTS;

#[tokio::test]
async fn test_modality_selection() {
    // Tests mapping extension and logic
    assert_eq!(
        EmbeddingSelector::select_modality("some python code", Some("py")),
        ContentModality::Code
    );
    assert_eq!(
        EmbeddingSelector::select_modality("some rust code", Some("rs")),
        ContentModality::Code
    );
    assert_eq!(
        EmbeddingSelector::select_modality("function example() {}", Some("js")),
        ContentModality::Code
    );
    assert_eq!(
        EmbeddingSelector::select_modality("# Title", Some("md")),
        ContentModality::Text
    );

    // Test code heuristics
    assert_eq!(
        EmbeddingSelector::select_modality("fn main() {}", None),
        ContentModality::Code
    );
    assert_eq!(
        EmbeddingSelector::select_modality("def hello_world():", None),
        ContentModality::Code
    );
    assert_eq!(
        EmbeddingSelector::select_modality("plain english text", None),
        ContentModality::Text
    );
}

#[tokio::test]
async fn test_selector_fallback() {
    // Create new embedding selector
    SYSTEM_ALERTS.clear();
    let selector = EmbeddingSelector::new().await.unwrap();

    let text_embedder = selector.get_embedder(ContentModality::Text).await;
    assert_eq!(text_embedder.dimension(), 768);

    // Default configuration uses offline Qwen for test, it should fallback to 768d text embedder
    let code_embedder = selector.get_embedder(ContentModality::Code).await;
    assert_eq!(code_embedder.dimension(), 768);

    let alerts = SYSTEM_ALERTS.get_alerts();
    let embedding_alerts: Vec<String> = alerts
        .into_iter()
        .filter(|a| a.component == "embedding")
        .map(|a| format!("[{}] {}", a.level, a.message))
        .collect();

    assert!(embedding_alerts
        .iter()
        .any(|msg| msg.contains("Qwen 1024d is offline, falling back to Text 768d")));
}

#[tokio::test]
async fn test_dimension_mismatch_rejection() {
    use rusqlite::Connection;
    use xavier::memory::multimodal_vec_store::{DataTypeKind, MultimodalVecManager};

    let manager = MultimodalVecManager::new();
    let conn = Connection::open_in_memory().unwrap();

    manager
        .ensure_modality_table(&conn, DataTypeKind::Text, 768)
        .unwrap();
    manager
        .ensure_modality_table(&conn, DataTypeKind::Code, 1024)
        .unwrap();

    let text_vec_bad = vec![0.0f32; 1024]; // Wrong dim for Text
    let code_vec_bad = vec![0.0f32; 512]; // Unhandled bad dim for Code

    let res_text =
        manager.insert_modality_embedding(&conn, DataTypeKind::Text, "text1", &text_vec_bad, "{}");
    assert!(res_text.is_err());
    assert!(res_text
        .unwrap_err()
        .to_string()
        .contains("expected 768 dimensions"));

    let res_code =
        manager.insert_modality_embedding(&conn, DataTypeKind::Code, "code1", &code_vec_bad, "{}");
    assert!(res_code.is_err());
    assert!(res_code
        .unwrap_err()
        .to_string()
        .contains("expected 1024 dimensions"));
}

#[tokio::test]
async fn test_dimension_fallback_routing() {
    use rusqlite::Connection;
    use xavier::memory::multimodal_vec_store::{DataTypeKind, MultimodalVecManager};

    let manager = MultimodalVecManager::new();
    let conn = Connection::open_in_memory().unwrap();

    manager
        .ensure_modality_table(&conn, DataTypeKind::Text, 768)
        .unwrap();
    manager
        .ensure_modality_table(&conn, DataTypeKind::Code, 1024)
        .unwrap();

    let fallback_code_vec = vec![0.0f32; 768];

    // Simulating inserting a Code snippet but with the fallback 768d vector
    let res = manager.insert_modality_embedding(
        &conn,
        DataTypeKind::Code,
        "code_snippet_fallback",
        &fallback_code_vec,
        "{}",
    );
    assert!(
        res.is_ok(),
        "Fallback 768d vector for Code modality should be stored successfully"
    );

    // It should route to the 768d (Text) table
    let search_res = manager
        .search_modality(&conn, DataTypeKind::Text, &fallback_code_vec, 1)
        .unwrap();
    assert_eq!(search_res.len(), 1);
    assert_eq!(search_res[0].item_id, "code_snippet_fallback");
}
