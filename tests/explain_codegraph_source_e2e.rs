//! feat-explain-codegraph-source: codegraph explain source e2e tests
use code_graph::db::CodeGraphDB;
use code_graph::query::QueryEngine;
use code_graph::types::{CodeEdge, EdgeType, Language, Symbol, SymbolKind};
use std::path::PathBuf;
use std::sync::Arc;
use xavier::explain::{CodeGraphSource, EvidenceKind, EvidenceSource, ExplainTarget};

xavier::isolate_test_process!();

fn target(file: &str, start_line: u32, end_line: u32, symbol: Option<&str>) -> ExplainTarget {
    ExplainTarget {
        repo_root: PathBuf::from("/nonexistent-repo-root"),
        file: PathBuf::from(file),
        start_line,
        end_line,
        symbol: symbol.map(str::to_string),
    }
}

fn sym(name: &str, file: &str, start_line: u32, end_line: u32) -> Symbol {
    Symbol {
        id: None,
        stable_id: None,
        name: name.to_string(),
        kind: SymbolKind::Function,
        lang: Language::Rust,
        file_path: file.to_string(),
        start_line,
        end_line,
        start_col: 0,
        end_col: 0,
        signature: Some(format!("fn {name}()")),
        parent: None,
        complexity: None,
    }
}

fn stable_id(db: &CodeGraphDB, name: &str) -> String {
    db.find_by_name(name, 1).unwrap()[0]
        .stable_id
        .clone()
        .unwrap()
}

fn call(db: &CodeGraphDB, from: &str, to: &str, file: &str, line: u32) {
    db.insert_edge(&CodeEdge {
        id: None,
        from_symbol: from.to_string(),
        to_symbol: to.to_string(),
        edge_type: EdgeType::Calls,
        file_path: file.to_string(),
        line,
        confidence: 1.0,
        metadata: None,
    })
    .unwrap();
}

#[test]
fn e2e_codegraph_source_callers_and_callees_of_symbol_in_range_get_source_refs() {
    let dir = tempfile::tempdir().unwrap();
    let db = CodeGraphDB::new(&dir.path().join("graph.db")).unwrap();
    for symbol in [
        sym("handle", "src/lib.rs", 2, 6),
        sym("route", "src/lib.rs", 10, 12),
        sym("unrelated", "src/lib.rs", 20, 22),
        sym("helper", "src/util.rs", 1, 3),
    ] {
        db.insert_symbol(&symbol).unwrap();
    }
    let handle = stable_id(&db, "handle");
    let route = stable_id(&db, "route");
    let unrelated = stable_id(&db, "unrelated");
    let helper = stable_id(&db, "helper");
    call(&db, &handle, &helper, "src/lib.rs", 4);
    call(&db, &route, &handle, "src/lib.rs", 11);
    call(&db, &unrelated, &helper, "src/lib.rs", 21);
    let engine = Arc::new(QueryEngine::new(Arc::new(db)));
    let source = CodeGraphSource::new(engine);

    let evidence = source
        .collect(&target("src/lib.rs", 3, 4, None), 10)
        .unwrap();

    assert_eq!(evidence.len(), 2);
    let caller = &evidence[0];
    assert_eq!(caller.kind, EvidenceKind::CallGraph);
    assert_eq!(caller.source_ref, "caller:route@src/lib.rs:11");
    assert_eq!(caller.text, "route -> handle (Calls)");
    let callee = &evidence[1];
    assert_eq!(callee.source_ref, "callee:helper@src/lib.rs:4");
    assert_eq!(callee.text, "handle -> helper (Calls)");
}

#[test]
fn e2e_codegraph_source_empty_file_or_range_returns_ok_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db = CodeGraphDB::new(&dir.path().join("graph.db")).unwrap();
    let engine = Arc::new(QueryEngine::new(Arc::new(db)));
    let source = CodeGraphSource::new(engine);

    let no_file = source
        .collect(&target("src/missing.rs", 1, 5, None), 10)
        .unwrap();
    let no_symbol = source
        .collect(&target("src/lib.rs", 50, 60, None), 10)
        .unwrap();

    assert!(no_file.is_empty());
    assert!(no_symbol.is_empty());
}

#[test]
fn e2e_codegraph_source_symbols_outside_line_range_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let db = CodeGraphDB::new(&dir.path().join("graph.db")).unwrap();
    for symbol in [
        sym("handle", "src/lib.rs", 2, 6),
        sym("route", "src/lib.rs", 10, 12),
        sym("unrelated", "src/lib.rs", 50, 60), // OUTSIDE range 3-4
        sym("helper", "src/util.rs", 1, 3),
    ] {
        db.insert_symbol(&symbol).unwrap();
    }
    let handle = stable_id(&db, "handle");
    let route = stable_id(&db, "route");
    let unrelated = stable_id(&db, "unrelated");
    let helper = stable_id(&db, "helper");
    call(&db, &handle, &helper, "src/lib.rs", 4);
    call(&db, &route, &handle, "src/lib.rs", 11);
    call(&db, &unrelated, &helper, "src/lib.rs", 55);
    let engine = Arc::new(QueryEngine::new(Arc::new(db)));
    let source = CodeGraphSource::new(engine);

    let evidence = source
        .collect(&target("src/lib.rs", 3, 4, None), 10)
        .unwrap();

    let refs: Vec<&str> = evidence.iter().map(|e| e.source_ref.as_str()).collect();
    assert_eq!(
        refs,
        vec!["caller:route@src/lib.rs:11", "callee:helper@src/lib.rs:4"]
    );
    assert!(!evidence.iter().any(|e| e.text.starts_with("unrelated")));
}
