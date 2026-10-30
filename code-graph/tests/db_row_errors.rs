use code_graph::db::CodeGraphDB;
use code_graph::types::{CodeEdge, EdgeType, Language, Symbol, SymbolKind};
use tempfile::TempDir;

#[test]
fn test_find_by_file_row_errors() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("graph.db");

    let db = CodeGraphDB::new(&db_path).unwrap();

    let sym = Symbol {
        id: None,
        stable_id: None,
        name: "test_sym".to_string(),
        kind: SymbolKind::Function,
        lang: Language::Rust,
        file_path: "src/test.rs".to_string(),
        start_line: 1,
        end_line: 2,
        start_col: 0,
        end_col: 0,
        signature: None,
        parent: None,
        complexity: None,
    };
    db.insert_symbol(&sym).unwrap();

    let found = db.find_by_file("src/test.rs").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "test_sym");

    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();
    // Insert malformed column values to trigger row decoding error
    raw_conn.execute(
        "INSERT INTO symbols (name, kind, lang, file_path, start_line, end_line, start_col, end_col)
         VALUES ('bad_sym', 'Function', 'Rust', 'src/test.rs', 'not_an_int', 'not_an_int', 0, 0)",
        []
    ).unwrap();

    // Assert find_by_file returns Err
    let result = db.find_by_file("src/test.rs");
    assert!(result.is_err());
}

#[test]
fn test_find_edges_to_row_errors() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("graph.db");

    let db = CodeGraphDB::new(&db_path).unwrap();

    let edge = CodeEdge {
        id: None,
        from_symbol: "src_sym".to_string(),
        to_symbol: "dst_sym".to_string(),
        edge_type: EdgeType::Calls,
        file_path: "src/test.rs".to_string(),
        line: 1,
        confidence: 1.0,
        metadata: None,
    };
    db.insert_edge(&edge).unwrap();

    let found = db.find_edges_to("dst_sym", None, 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].from_symbol, "src_sym");

    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();
    // Insert malformed column values to trigger row decoding error
    raw_conn
        .execute(
            "INSERT INTO edges (from_symbol, to_symbol, edge_type, file_path, line, confidence)
         VALUES ('bad_src', 'dst_sym', 'Calls', 'src/test.rs', 'not_an_int', 1.0)",
            [],
        )
        .unwrap();

    let result = db.find_edges_to("dst_sym", None, 10);
    assert!(result.is_err());
}
