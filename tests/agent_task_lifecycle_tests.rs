use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::agents::{Agent, AgentConfig};
use xavier::coordination::{KeyLendingEngine, SimpleAgentRegistry, XavierEventBus};
use xavier::memory::qmd_memory::{MemoryDocument, QmdMemory};
use xavier::secrets::audit::QmdAuditLogger;

#[tokio::test]
async fn test_agent_task_lifecycle_hooks() -> Result<()> {
    // 1. Setup engines
    let event_bus = XavierEventBus::new(100);
    let secrets_engine = Arc::new(KeyLendingEngine::new(
        Box::new(QmdAuditLogger::new()),
        Some(event_bus.clone()),
    ));

    // 2. Setup registry with engines
    let registry = SimpleAgentRegistry::new_with_engines(
        Some(secrets_engine.clone()),
        Some(event_bus.clone()),
    );

    // 3. Register an agent and lend a secret
    let agent_id = "test-agent-lifecycle";
    registry
        .register(
            agent_id.to_string(),
            "session-1".to_string(),
            Default::default(),
        )
        .await;

    let lease = secrets_engine
        .lend("API_KEY", Some("sk-123"), agent_id, 10)
        .await?;
    let token = lease.token.clone();

    // Verify lease exists and has short TTL
    let initial_lease = secrets_engine.get_lease(&token).await.unwrap();
    let _initial_expires = initial_lease.expires_at;

    // 4. Setup Agent and Memory
    let config = AgentConfig::new(agent_id.to_string()).with_task("Test task".to_string());
    let mut agent = Agent::new(config);

    let docs = Arc::new(RwLock::new(Vec::<MemoryDocument>::new()));
    let memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws".to_string()));

    // 5. Run agent with lifecycle hooks
    // This should trigger on_task_start (renewing lease) and on_task_complete (revoking lease)
    let _ = agent.run(memory, Some(registry.clone())).await?;

    // 6. Verify lease was revoked after task completion
    let final_lease = secrets_engine.get_lease(&token).await;
    assert!(
        final_lease.is_none(),
        "Lease should have been revoked after task completion"
    );

    Ok(())
}

#[tokio::test]
async fn test_on_task_start_renews_lease() -> Result<()> {
    // 1. Setup engines
    let event_bus = XavierEventBus::new(100);
    let secrets_engine = Arc::new(KeyLendingEngine::new(
        Box::new(QmdAuditLogger::new()),
        Some(event_bus.clone()),
    ));

    // 2. Setup registry
    let registry = SimpleAgentRegistry::new_with_engines(
        Some(secrets_engine.clone()),
        Some(event_bus.clone()),
    );

    let agent_id = "test-agent-start";
    let task_id = "task-123";

    // 3. Lend a secret with 10s TTL
    let lease = secrets_engine
        .lend("API_KEY", Some("sk-123"), agent_id, 10)
        .await?;
    let token = lease.token.clone();
    let initial_expires = lease.expires_at;

    // 4. Trigger on_task_start manually via port
    use xavier::ports::inbound::AgentLifecyclePort;
    registry.on_task_start(agent_id, task_id).await;

    // 5. Verify lease was renewed (expires_at should be roughly now + 3600s)
    let updated_lease = secrets_engine.get_lease(&token).await.unwrap();
    assert!(updated_lease.expires_at > initial_expires + chrono::Duration::seconds(3500));

    Ok(())
}
