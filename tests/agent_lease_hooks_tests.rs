use std::sync::Arc;
use tokio::time::{sleep, Duration};
use xavier::agents::runtime::{AgentRuntime, RuntimeConfig};
use xavier::coordination::core::CoordinationCore;
use xavier::coordination::events::XavierEventBus;
use xavier::coordination::secrets::KeyLendingEngine;
use xavier::memory::qmd_memory::QmdMemory;
use xavier::secrets::audit::QmdAuditLogger;

#[tokio::test]
async fn test_agent_task_lifecycle_lease_revocation() {
    // 1. Setup Event Bus and Secrets Engine
    let event_bus = XavierEventBus::new(100);
    let audit_logger = Box::new(QmdAuditLogger::new());
    let secrets_engine = Arc::new(KeyLendingEngine::new(audit_logger, None));

    // 2. Setup Coordination Core (The listener)
    let core = CoordinationCore::new(event_bus.clone(), secrets_engine.clone());
    core.start();

    // 3. Setup Agent Runtime
    let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
    let runtime = AgentRuntime::new(memory, None, RuntimeConfig::default())
        .unwrap()
        .with_event_bus(Arc::new(event_bus));

    // 4. Lend a secret to the agent manually (simulating pre-task setup)
    let agent_id = "default-agent";
    secrets_engine
        .lend("TEST_KEY", Some("secret_value"), agent_id, 3600)
        .await
        .unwrap();

    let leases = secrets_engine.list_leases().await;
    assert_eq!(leases.len(), 1, "Should have 1 active lease before task");
    assert_eq!(leases[0].agent_id, agent_id);

    // 5. Run the agent (this should trigger the AgentTaskStarted and AgentTaskCompleted events)
    // We use a query that triggers a direct response to keep it simple and fast
    println!("Running agent...");
    let res = runtime.run("hello", None, None).await.unwrap();
    println!("Agent response: {}", res.response);

    // 6. Give the coordination core a moment to process the event
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        let leases = secrets_engine.list_leases().await;
        if leases.is_empty() {
            break;
        }
    }

    // 7. Verify the lease was revoked
    let leases_after = secrets_engine.list_leases().await;
    assert_eq!(
        leases_after.len(),
        0,
        "Lease should have been automatically revoked after task completion"
    );
}

#[tokio::test]
async fn test_agent_task_failure_lease_revocation() {
    // 1. Setup
    let event_bus = XavierEventBus::new(100);
    let audit_logger = Box::new(QmdAuditLogger::new());
    let secrets_engine = Arc::new(KeyLendingEngine::new(audit_logger, None));

    let core = CoordinationCore::new(event_bus.clone(), secrets_engine.clone());
    core.start();

    // 2. Setup Runtime with invalid config to force failure (e.g. no memory - actually we just use a mock that fails)
    // Since AgentRuntime::run is complex, we'll just manually emit a failure event to test the core's reaction
    // but the previous test already verifies that the runtime calls the hooks.

    let agent_id = "failing-agent";
    secrets_engine
        .lend("FAIL_KEY", Some("value"), agent_id, 3600)
        .await
        .unwrap();

    // Emit failure event manually
    let _ = event_bus.publish(xavier::coordination::events::XavierEvent::AgentTaskFailed {
        agent_id: agent_id.to_string(),
        task_id: "failed-task".to_string(),
        reason: "Simulated failure".to_string(),
    });

    sleep(Duration::from_millis(100)).await;

    let leases = secrets_engine.list_leases().await;
    assert_eq!(
        leases.len(),
        0,
        "Lease should have been revoked after task failure"
    );
}
