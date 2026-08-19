use rusqlite::{params, Connection};
use std::fs;
use tempfile::tempdir;
use xavier::memory::sqlite_vec_store::vector::serialize_embedding;
use xavier::storage::export::{ExportFormat, VectorExportRecord, VectorExporter};

fn setup_test_db(db_path: &std::path::Path) -> Connection {
    let conn = Connection::open(db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE memory_records (
            id TEXT PRIMARY KEY,
            workspace_id TEXT,
            path TEXT,
            content TEXT,
            metadata TEXT,
            embedding BLOB,
            created_at TEXT,
            updated_at TEXT
        );
        CREATE TABLE memory_embeddings (
            id TEXT PRIMARY KEY,
            workspace_id TEXT,
            embedding BLOB
        );",
    )
    .unwrap();
    conn
}

#[tokio::test]
async fn test_export_jsonl_structure_and_formatting() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test_vec.db");
    let conn = setup_test_db(&db_path);

    let vec1 = vec![0.1f32, 0.2f32, -0.3f32, 0.4f32];
    let vec2 = vec![1.0f32, 2.5f32, -3.25f32];

    let emb1_blob = serialize_embedding(&vec1);
    let emb2_blob = serialize_embedding(&vec2);

    conn.execute(
        "INSERT INTO memory_records (id, workspace_id, path, content, metadata, embedding, created_at, updated_at) \
         VALUES ('rec_1', 'ws_default', 'doc/1', 'First document', '{\"tag\":\"a\"}', ?1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        params![emb1_blob],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO memory_records (id, workspace_id, path, content, metadata, embedding, created_at, updated_at) \
         VALUES ('rec_2', 'ws_default', 'doc/2', 'Second document', '{\"tag\":\"b\"}', ?1, '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z')",
        params![emb2_blob],
    )
    .unwrap();

    let exporter = VectorExporter::new(&db_path);
    let jsonl_path = dir.path().join("export.jsonl");

    let count = exporter.export_jsonl(&jsonl_path).await.unwrap();
    assert_eq!(count, 2);

    let jsonl_content = fs::read_to_string(&jsonl_path).unwrap();
    let lines: Vec<&str> = jsonl_content.lines().collect();
    assert_eq!(lines.len(), 2);

    let rec1: VectorExportRecord = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(rec1.id, "rec_1");
    assert_eq!(rec1.workspace_id, "ws_default");
    assert_eq!(rec1.path, "doc/1");
    assert_eq!(rec1.content, "First document");
    assert_eq!(rec1.metadata["tag"], "a");
    assert_eq!(rec1.embedding, vec1);

    let rec2: VectorExportRecord = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(rec2.id, "rec_2");
    assert_eq!(rec2.workspace_id, "ws_default");
    assert_eq!(rec2.path, "doc/2");
    assert_eq!(rec2.content, "Second document");
    assert_eq!(rec2.metadata["tag"], "b");
    assert_eq!(rec2.embedding, vec2);
}

#[tokio::test]
async fn test_export_streaming_buffer_bounds() {
    let dir = tempdir().unwrap();
    let db_path = db_path_for_streaming(&dir);

    let exporter = VectorExporter::new(&db_path).with_batch_size(500);
    let jsonl_path = dir.path().join("streaming.jsonl");

    let count = exporter.export_jsonl(&jsonl_path).await.unwrap();
    assert_eq!(count, 1200);

    let content = fs::read_to_string(&jsonl_path).unwrap();
    let lines_count = content.lines().count();
    assert_eq!(lines_count, 1200);
}

fn db_path_for_streaming(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let db_path = dir.path().join("stream_test.db");
    let conn = setup_test_db(&db_path);

    let sample_emb = serialize_embedding(&[0.5f32, -0.5f32, 1.5f32]);
    let tx = conn.unchecked_transaction().unwrap();

    for i in 0..1200 {
        let id = format!("mem_{:04}", i);
        let path = format!("path/{:04}", i);
        tx.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, metadata, embedding) \
             VALUES (?1, 'ws_stream', ?2, 'Streaming content', '{\"batch\":true}', ?3)",
            params![id, path, sample_emb],
        )
        .unwrap();
    }

    tx.commit().unwrap();
    db_path
}

#[tokio::test]
async fn test_export_parquet_magic_header_and_footer() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("parquet_test.db");
    let conn = setup_test_db(&db_path);

    let sample_emb = serialize_embedding(&[0.1f32, 0.2f32]);
    conn.execute(
        "INSERT INTO memory_records (id, workspace_id, path, content, metadata, embedding) \
         VALUES ('p_1', 'ws_p', 'p/1', 'Parquet content', '{}', ?1)",
        params![sample_emb],
    )
    .unwrap();

    let exporter = VectorExporter::new(&db_path);
    let parquet_path = dir.path().join("export.parquet");

    let count = exporter
        .export(&parquet_path, ExportFormat::Parquet)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let file_bytes = fs::read(&parquet_path).unwrap();
    assert!(file_bytes.len() >= 8);
    assert_eq!(&file_bytes[..4], b"PAR1");
    assert_eq!(&file_bytes[file_bytes.len() - 4..], b"PAR1");
}

#[tokio::test]
async fn test_export_empty_db() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("empty.db");
    let exporter = VectorExporter::new(&db_path);

    let jsonl_path = dir.path().join("empty.jsonl");
    let count = exporter.export_jsonl(&jsonl_path).await.unwrap();
    assert_eq!(count, 0);

    let content = fs::read_to_string(&jsonl_path).unwrap();
    assert!(content.is_empty());
}
