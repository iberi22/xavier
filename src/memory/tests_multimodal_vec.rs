//! Unit tests for Multimodal Virtual Vector Manager
//!
//! Tests isolated virtual vector tables per data modality using in-memory SQLite connections.

use anyhow::Result;
use rusqlite::Connection;

use crate::memory::multimodal_vec_store::{
    compute_cosine_distance, DataTypeKind, MultimodalVecManager, VectorSearchResult,
};

fn setup_test_conn() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    Ok(conn)
}

#[test]
fn test_modality_table_initialization() -> Result<()> {
    let conn = setup_test_conn()?;
    let manager = MultimodalVecManager::new();

    // Initialize tables for different modalities with distinct dimensions
    manager.ensure_modality_table(&conn, DataTypeKind::Code, 384)?;
    manager.ensure_modality_table(&conn, DataTypeKind::Legal, 1536)?;
    manager.ensure_modality_table(&conn, DataTypeKind::Image, 768)?;

    // Verify stored dimensions
    assert_eq!(manager.get_modality_dim(&conn, DataTypeKind::Code)?, 384);
    assert_eq!(manager.get_modality_dim(&conn, DataTypeKind::Legal)?, 1536);
    assert_eq!(manager.get_modality_dim(&conn, DataTypeKind::Image)?, 768);

    Ok(())
}

#[test]
fn test_dimension_mismatch_validation() -> Result<()> {
    let conn = setup_test_conn()?;
    let manager = MultimodalVecManager::new();

    manager.ensure_modality_table(&conn, DataTypeKind::Code, 384)?;

    // Attempt insert with 3-dim vector into 384-dim table
    let invalid_vec = vec![0.1f32, 0.2f32, 0.3f32];
    let res = manager.insert_modality_embedding(
        &conn,
        DataTypeKind::Code,
        "item_err",
        &invalid_vec,
        r#"{"key":"val"}"#,
    );

    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("Dimension mismatch"),
        "Error message should mention dimension mismatch, got: {}",
        err_msg
    );

    // Attempt search with mismatched query vector
    let search_res = manager.search_modality(&conn, DataTypeKind::Code, &invalid_vec, 5);
    assert!(search_res.is_err());
    assert!(search_res.unwrap_err().to_string().contains("Query dimension mismatch"));

    Ok(())
}

#[test]
fn test_multimodal_vector_isolation_and_knn_search() -> Result<()> {
    let conn = setup_test_conn()?;
    let manager = MultimodalVecManager::new();

    // Use small dimensions for fast testing
    manager.ensure_modality_table(&conn, DataTypeKind::Code, 3)?;
    manager.ensure_modality_table(&conn, DataTypeKind::Legal, 4)?;

    // Insert into Code table (3d)
    let code_v1 = vec![1.0f32, 0.0f32, 0.0f32];
    let code_v2 = vec![0.8f32, 0.2f32, 0.0f32];
    manager.insert_modality_embedding(
        &conn,
        DataTypeKind::Code,
        "code_item_1",
        &code_v1,
        r#"{"lang":"rust"}"#,
    )?;
    manager.insert_modality_embedding(
        &conn,
        DataTypeKind::Code,
        "code_item_2",
        &code_v2,
        r#"{"lang":"python"}"#,
    )?;

    // Insert into Legal table (4d)
    let legal_v1 = vec![0.0f32, 1.0f32, 0.0f32, 0.0f32];
    manager.insert_modality_embedding(
        &conn,
        DataTypeKind::Legal,
        "legal_item_1",
        &legal_v1,
        r#"{"doc":"contract"}"#,
    )?;

    // Search Code table
    let query_code = vec![1.0f32, 0.0f32, 0.0f32];
    let code_results = manager.search_modality(&conn, DataTypeKind::Code, &query_code, 10)?;

    assert_eq!(code_results.len(), 2);
    assert_eq!(code_results[0].item_id, "code_item_1");
    assert!(code_results[0].distance < code_results[1].distance);

    // Verify Legal table items are isolated and not returned in Code search
    assert!(!code_results.iter().any(|r| r.item_id == "legal_item_1"));

    // Search Legal table
    let query_legal = vec![0.0f32, 1.0f32, 0.0f32, 0.0f32];
    let legal_results = manager.search_modality(&conn, DataTypeKind::Legal, &query_legal, 10)?;

    assert_eq!(legal_results.len(), 1);
    assert_eq!(legal_results[0].item_id, "legal_item_1");
    assert_eq!(legal_results[0].payload_json, r#"{"doc":"contract"}"#);

    Ok(())
}

#[test]
fn test_payload_retrieval_and_updates() -> Result<()> {
    let conn = setup_test_conn()?;
    let manager = MultimodalVecManager::new();

    manager.ensure_modality_table(&conn, DataTypeKind::Audio, 2)?;

    let v1 = vec![0.5f32, 0.5f32];
    manager.insert_modality_embedding(
        &conn,
        DataTypeKind::Audio,
        "audio_1",
        &v1,
        r#"{"sample_rate":44100}"#,
    )?;

    let results = manager.search_modality(&conn, DataTypeKind::Audio, &v1, 1)?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].payload_json, r#"{"sample_rate":44100}"#);

    // Update existing item with new vector and payload
    let v2 = vec![1.0f32, 0.0f32];
    manager.insert_modality_embedding(
        &conn,
        DataTypeKind::Audio,
        "audio_1",
        &v2,
        r#"{"sample_rate":48000}"#,
    )?;

    let updated_results = manager.search_modality(&conn, DataTypeKind::Audio, &v2, 1)?;
    assert_eq!(updated_results.len(), 1);
    assert_eq!(updated_results[0].item_id, "audio_1");
    assert_eq!(updated_results[0].payload_json, r#"{"sample_rate":48000}"#);

    Ok(())
}

#[test]
fn test_compute_cosine_distance_unit() {
    let v1 = vec![1.0f32, 0.0f32, 0.0f32];
    let v2 = vec![1.0f32, 0.0f32, 0.0f32];
    let v3 = vec![0.0f32, 1.0f32, 0.0f32];
    let v4 = vec![-1.0f32, 0.0f32, 0.0f32];

    let d_identical = compute_cosine_distance(&v1, &v2);
    assert!((d_identical - 0.0).abs() < 1e-5);

    let d_orthogonal = compute_cosine_distance(&v1, &v3);
    assert!((d_orthogonal - 1.0).abs() < 1e-5);

    let d_opposite = compute_cosine_distance(&v1, &v4);
    assert!((d_opposite - 2.0).abs() < 1e-5);
}
