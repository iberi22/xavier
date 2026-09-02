//! Benchmarks for code-graph

#[cfg(test)]
mod benchmarks_inner {
    use crate::db::CodeGraphDB;
    use crate::types::{Language, Symbol, SymbolKind};
    use std::time::Instant;

    fn setup_large_db() -> CodeGraphDB {
        let db = CodeGraphDB::in_memory().expect("benchmark assertion");

        // Insert 1000 symbols for benchmarking
        for i in 0..1000 {
            let sym = Symbol {
                id: None,
                stable_id: None,
                name: format!("function_{}", i),
                kind: SymbolKind::Function,
                lang: Language::Rust,
                file_path: format!("/src/module_{}/file{}.rs", i % 10, i),
                start_line: i as u32,
                end_line: (i + 10) as u32,
                start_col: 0,
                end_col: 0,
                signature: Some(format!("fn function_{}() -> Result<()>", i)),
                parent: None,
                complexity: None,
            };
            db.insert_symbol(&sym).expect("benchmark assertion");
        }

        db
    }

    #[test]
    fn benchmark_search_exact() {
        let db = setup_large_db();

        let start = Instant::now();
        for _ in 0..100 {
            db.find_symbols("function_500", 10)
                .expect("benchmark assertion");
        }
        let elapsed = start.elapsed();

        println!("Exact search (100 queries): {:?}", elapsed);
        assert!(elapsed.as_millis() < 1000);
    }

    #[test]
    fn benchmark_search_fuzzy() {
        let db = setup_large_db();

        let start = Instant::now();
        for _ in 0..100 {
            db.find_symbols("function_", 10)
                .expect("benchmark assertion");
        }
        let elapsed = start.elapsed();

        println!("Fuzzy search (100 queries): {:?}", elapsed);
        assert!(elapsed.as_millis() < 2000);
    }

    #[test]
    fn benchmark_insert() {
        let db = CodeGraphDB::in_memory().expect("benchmark assertion");

        let start = Instant::now();
        for i in 0..100 {
            let sym = Symbol {
                id: None,
                stable_id: None,
                name: format!("bench_{}", i),
                kind: SymbolKind::Function,
                lang: Language::Rust,
                file_path: "/src/main.rs".to_string(),
                start_line: 1,
                end_line: 10,
                start_col: 0,
                end_col: 0,
                signature: Some("fn bench()".to_string()),
                parent: None,
                complexity: None,
            };
            db.insert_symbol(&sym).expect("benchmark assertion");
        }
        let elapsed = start.elapsed();

        println!("Insert 100 symbols: {:?}", elapsed);
        assert!(elapsed.as_millis() < 500);
    }

    #[test]
    fn benchmark_find_by_kind() {
        let db = setup_large_db();

        let start = Instant::now();
        for _ in 0..100 {
            db.find_by_kind(SymbolKind::Function, 100)
                .expect("benchmark assertion");
        }
        let elapsed = start.elapsed();

        println!("Find by kind (100 queries): {:?}", elapsed);
        assert!(elapsed.as_millis() < 1000);
    }

    #[test]
    fn benchmark_batch_insert_wal_guard() {
        let temp_dir = tempfile::tempdir().expect("benchmark tempdir");
        let db_path = temp_dir.path().join("bench_wal.db");
        let db = CodeGraphDB::new(&db_path).expect("open file db");

        let symbols: Vec<Symbol> = (0..500)
            .map(|i| Symbol {
                id: None,
                stable_id: None,
                name: format!("bench_sym_{}", i),
                kind: SymbolKind::Function,
                lang: Language::Rust,
                file_path: format!("/src/file_{}.rs", i % 10),
                start_line: i as u32,
                end_line: (i + 1) as u32,
                start_col: 0,
                end_col: 0,
                signature: Some("fn bench()".to_string()),
                parent: None,
                complexity: None,
            })
            .collect();

        // Verify journal_size_limit PRAGMA setting directly on CodeGraphDB connection
        {
            let conn = db.conn.lock().expect("lock db conn");
            let pragma_value: i64 = conn
                .query_row("PRAGMA journal_size_limit;", [], |row| row.get(0))
                .expect("query pragma journal_size_limit");
            assert_eq!(pragma_value, 67108864);
        }

        db.insert_symbols(&symbols).expect("batch insert");
        db.checkpoint_wal().expect("checkpoint wal");

        let wal_path = db_path.with_extension("db-wal");
        if wal_path.exists() {
            let wal_size = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
            assert!(
                wal_size < 64 * 1024 * 1024,
                "WAL file size {} exceeds 64MB limit",
                wal_size
            );
        }
    }
}
