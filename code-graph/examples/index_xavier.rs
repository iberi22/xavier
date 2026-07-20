fn main() {
    println!("Indexing Xavier source code...");
    
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let path = std::path::Path::new("src");
        let db_path = std::path::Path::new("data/code_graph.db");
        
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        
        let db = std::sync::Arc::new(code_graph::db::CodeGraphDB::new(db_path).unwrap());
        let indexer = code_graph::indexer::Indexer::new(db);
        
        match indexer.index(path).await {
            Ok(stats) => {
                println!("Indexing complete:");
                println!("  Files: {}", stats.total_files);
                println!("  Symbols: {}", stats.total_symbols);
                println!("  Duration: {}ms", stats.duration_ms);
            }
            Err(e) => {
                eprintln!("Indexing failed: {}", e);
            }
        }
    });
}
