//! feat-explain-memory-source: explain memory source e2e tests
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::explain::{EvidenceSource, ExplainTarget, MemorySource, QmdMemorySearch};
use xavier::memory::qmd::QmdMemory;
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::memory::schema::MemoryKind;

xavier::isolate_test_process!();

fn target() -> ExplainTarget {
    ExplainTarget {
        repo_root: PathBuf::from("/nonexistent-repo"),
        file: PathBuf::from("src/lib.rs"),
        start_line: 2,
        end_line: 3,
        symbol: Some("demo".to_string()),
    }
}

fn doc(id: &str, content: &str, kind: MemoryKind, path: &str) -> MemoryDocument {
    let mut doc = MemoryDocument::default();
    doc.id = Some(id.to_string());
    doc.content = content.to_string();
    doc.path = path.to_string();
    let meta = serde_json::json!({ "kind": kind, "provenance": {"file_path": path} });
    doc.metadata = serde_json::Value::String(serde_json::to_string(&meta).unwrap());
    doc
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn e2e_memory_source_real_adapter_decision_query_and_deduplication() {
    let _env = xavier::settings::XavierSettings::default();
    let _dir = tempfile::tempdir().unwrap();
    std::env::set_var("XAVIER_DATA_DIR", _dir.path());
    std::env::set_var("XAVIER_WORKSPACE_DIR", "ws-test");

    let docs = vec![
        doc(
            "shared",
            "src/lib.rs demo decision text",
            MemoryKind::Decision,
            "src/lib.rs",
        ),
        doc(
            "other",
            "src/lib.rs demo other memory",
            MemoryKind::Episodic,
            "src/lib.rs",
        ),
        doc(
            "unrelated",
            "foo_bar string not matching",
            MemoryKind::Episodic,
            "other_file.rs",
        ), // Does not mention src/lib.rs or demo
    ];
    // QmdMemory::new_with_workspace is memory-backed only if initialized this way
    let memory = Arc::new(QmdMemory::new_with_workspace(
        Arc::new(RwLock::new(docs)),
        "ws-test",
    ));
    let search = Arc::new(QmdMemorySearch::new(memory));
    let source = MemorySource::new(search, tokio::runtime::Handle::current());

    // Run blocking wrapper inside spawn_blocking
    let evidence = tokio::task::spawn_blocking(move || source.collect(&target(), 5))
        .await
        .unwrap()
        .unwrap();

    let shared: Vec<_> = evidence
        .iter()
        .filter(|e| e.source_ref == "memory:shared")
        .collect();
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].text, "src/lib.rs demo decision text");
    assert!(evidence.iter().any(|e| e.source_ref == "memory:other"));
    assert!(!evidence.iter().any(|e| e.source_ref == "memory:unrelated"));
}
