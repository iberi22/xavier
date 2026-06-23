use anyhow::Result;
use xavier::memory::store::{MemoryRecord, MemoryStore};
use xavier::memory::sqlite_store::{SqliteMemoryStore, SqliteStoreConfig};
use tempfile::tempdir;

#[tokio::test]
async fn test_sqlite_fts5_search() -> Result<()> {
    let dir = tempdir()?;
    let db_path = dir.path().join("test_memory.db");

    let config = SqliteStoreConfig {
        path: db_path.clone(),
    };

    let store = SqliteMemoryStore::new(config).await?;

    let mut rec1 = MemoryRecord::default();
    rec1.id = "doc1_id".to_string();
    rec1.workspace_id = "ws1".to_string();
    rec1.path = "path/to/doc1.md".to_string();
    rec1.content = "The quick brown fox jumps over the lazy dog".to_string();

    let mut rec2 = MemoryRecord::default();
    rec2.id = "doc2_id".to_string();
    rec2.workspace_id = "ws1".to_string();
    rec2.path = "path/to/doc2.md".to_string();
    rec2.content = "Lazy dogs are really lazy, unlike fast foxes".to_string();

    store.put(rec1.clone()).await?;
    store.put(rec2.clone()).await?;

    // Search for 'fox'
    let results = store.search("ws1", "fox", None).await?;
    println!("Search 'fox' results: {:?}", results.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
    assert!(!results.is_empty());

    // Search for 'lazy' - record 2 should be first due to higher frequency
    let results = store.search("ws1", "lazy", None).await?;
    println!("Search 'lazy' results: {:?}", results.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
    assert!(results.len() >= 2);
    assert_eq!(results[0].id, "doc2_id");

    // Delete record 1
    println!("Deleting record 1...");
    let del_res = store.delete("ws1", "doc1_id").await?;
    println!("Deleted record: {:?}", del_res.as_ref().map(|r| r.id.clone()));
    assert!(del_res.is_some());

    // Search for 'fox' again
    let results = store.search("ws1", "fox", None).await?;
    println!("Search 'fox' after delete results: {:?}", results.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
    assert!(results.iter().all(|r| r.id != "doc1_id"));

    Ok(())
}
