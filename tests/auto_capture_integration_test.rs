use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::coordination::events::XavierEventBus;
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::coordination::events::XavierEvent;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn test_auto_capture_subscriber_logic() {
    let event_bus = XavierEventBus::new(10);
    let mut receiver = event_bus.subscribe();

    // Setup a temporary memory store
    let config = VecSqliteStoreConfig {
        path: std::env::temp_dir().join(format!("test_auto_capture_{}.db", ulid::Ulid::new())),
        embedding_dimensions: 1536,
    };
    let store = Arc::new(VecSqliteMemoryStore::new(config).await.expect("failed to create store"));
    let memory = Arc::new(QmdMemory::new_with_workspace(Arc::new(RwLock::new(Vec::new())), "test-ws".to_string()));
    memory.set_store(store.clone()).await;

    let memory_for_bus = Arc::clone(&memory);

    // Spawn the subscriber logic (similar to src/cli/server.rs)
    tokio::spawn(async move {
        while let Ok(event) = receiver.recv().await {
            match event {
                XavierEvent::ToolCalled { name, args, session_id: _ } => {
                    let path = format!("events/tool_call/{}/{}", name, ulid::Ulid::new());
                    let content = format!("Agent called tool: {} with args: {}", name, args);
                    let typed = xavier::memory::schema::TypedMemoryPayload {
                        kind: Some(xavier::memory::schema::MemoryKind::Episodic),
                        ..Default::default()
                    };
                    let _ = memory_for_bus.add_document_typed(path, content, args, Some(typed)).await;
                }
                _ => {}
            }
        }
    });

    // Publish an event
    event_bus.publish(XavierEvent::ToolCalled {
        name: "test_tool".to_string(),
        args: json!({"arg1": "val1"}),
        session_id: "test-session".to_string(),
    }).expect("failed to publish");

    // Give it a moment to process
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Verify memory contains the event
    let docs = memory.all_documents().await;
    let found = docs.iter().any(|d| d.content.contains("test_tool") && d.content.contains("val1"));
    assert!(found, "Should find the auto-captured tool call in memory. Found {} docs.", docs.len());
}
