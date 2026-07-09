#[cfg(test)]
mod tests {
    use crate::db::CodeGraphDB;
    use crate::indexer::Indexer;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn watcher_reindexes_single_file_on_change() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("main.rs");
        std::fs::write(&file_path, "fn old() {}").unwrap();

        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Arc::new(Indexer::new(db.clone()));

        // Initial full index
        indexer.index(dir.path(), true).await.unwrap();
        assert_eq!(db.stats().unwrap().total_symbols, 1);

        // Change file
        // Sleep to ensure mtime changes if needed (though std::fs::write should handle it)
        std::thread::sleep(std::time::Duration::from_millis(100));
        std::fs::write(&file_path, "fn old() {}\nfn new_function() {}").unwrap();

        // Re-index single file
        indexer.reindex_file(dir.path(), &file_path).await.unwrap();

        let stats = db.stats().unwrap();
        assert_eq!(stats.total_symbols, 2);
        assert_eq!(stats.total_files, 1);
    }

    #[tokio::test]
    async fn watcher_handles_new_file_creation() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Arc::new(Indexer::new(db.clone()));

        indexer.index(dir.path(), true).await.unwrap();
        assert_eq!(db.stats().unwrap().total_files, 1);

        // Create new file
        let lib_path = dir.path().join("lib.rs");
        std::fs::write(&lib_path, "pub fn helper() {}").unwrap();
        indexer.reindex_file(dir.path(), &lib_path).await.unwrap();

        assert_eq!(db.stats().unwrap().total_files, 2);
    }

    #[tokio::test]
    async fn watcher_handles_file_deletion() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        let lib_path = dir.path().join("lib.rs");
        std::fs::write(&lib_path, "fn helper() {}").unwrap();

        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Arc::new(Indexer::new(db.clone()));

        indexer.index(dir.path(), true).await.unwrap();
        assert_eq!(db.stats().unwrap().total_files, 2);

        // Delete file
        std::fs::remove_file(&lib_path).unwrap();
        indexer.reindex_file(dir.path(), &lib_path).await.unwrap();

        assert_eq!(db.stats().unwrap().total_files, 1);
    }
}
