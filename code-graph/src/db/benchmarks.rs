//! Benchmarks for code-graph

#[cfg(test)]
mod benchmarks_inner {
    use crate::db::CodeGraphDB;
    use crate::types::{Language, Symbol, SymbolKind};
    use std::time::Instant;

    fn setup_large_db(count: usize) -> CodeGraphDB {
        let db = CodeGraphDB::in_memory().expect("benchmark assertion");

        let mut symbols = Vec::with_capacity(count);
        for i in 0..count {
            symbols.push(Symbol {
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
            });
        }
        db.insert_symbols(&symbols).expect("benchmark assertion");

        db
    }

    #[test]
    fn benchmark_search_exact() {
        let db = setup_large_db(1000);

        let start = Instant::now();
        for _ in 0..100 {
            db.find_symbols("function_500", 10)
                .expect("benchmark assertion");
        }
        let elapsed = start.elapsed();

        println!("Exact search (1000 symbols, 100 queries): {:?}", elapsed);
        assert!(elapsed.as_millis() < 1000);
    }

    #[test]
    fn benchmark_search_fuzzy() {
        let db = setup_large_db(1000);

        let start = Instant::now();
        for _ in 0..100 {
            db.find_symbols("function_", 10)
                .expect("benchmark assertion");
        }
        let elapsed = start.elapsed();

        println!("Fuzzy search (1000 symbols, 100 queries): {:?}", elapsed);
        assert!(elapsed.as_millis() < 2000);
    }

    #[test]
    fn benchmark_search_large_scale() {
        // Benchmark with 50,000 symbols to match requirements
        let db = setup_large_db(50_000);

        let start = Instant::now();
        for i in 0..100 {
            let query = format!("function_{}", i * 500);
            db.find_symbols(&query, 10)
                .expect("benchmark assertion");
        }
        let elapsed = start.elapsed();

        println!("Large scale search (50,000 symbols, 100 queries): {:?}", elapsed);
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
        let db = setup_large_db(1000);

        let start = Instant::now();
        for _ in 0..100 {
            db.find_by_kind(SymbolKind::Function, 100)
                .expect("benchmark assertion");
        }
        let elapsed = start.elapsed();

        println!("Find by kind (100 queries): {:?}", elapsed);
        assert!(elapsed.as_millis() < 1000);
    }
}
