use xavier::notifications::{IslandId, NOTIFICATIONS};
use xavier::server::alerts::SYSTEM_ALERTS;

#[tokio::test]
async fn test_system_alert_pushes_notification() {
    let mut rx = NOTIFICATIONS.subscribe();

    SYSTEM_ALERTS.push_alert("ERROR", "Database connection lost", "storage");

    let notification = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for notification")
        .expect("failed to receive notification");

    assert_eq!(notification.island_id.as_str(), "errors");
    assert_eq!(notification.severity, "error");
    assert_eq!(notification.title, "[STORAGE] ERROR");
    assert_eq!(notification.body, "Database connection lost");
}

#[tokio::test]
async fn test_multi_island_events_and_persistence() {
    let mut rx = NOTIFICATIONS.subscribe();

    // 1. Emit System island event
    let sys_n = NOTIFICATIONS
        .notify(IslandId::System, "System Event", "CPU usage at 95%", "warning")
        .await
        .expect("notify system failed");
    assert_eq!(sys_n.island_id.as_str(), "system");

    let rx_sys = rx.recv().await.unwrap();
    assert_eq!(rx_sys.id, sys_n.id);

    // 2. Emit Memory island event
    let mem_n = NOTIFICATIONS
        .notify(
            IslandId::Memory,
            "Memory Sync Completed",
            "Synced with http://peer1:8080: sent 10 chunks, received 5 chunks (120ms)",
            "info",
        )
        .await
        .expect("notify memory failed");
    assert_eq!(mem_n.island_id.as_str(), "memory");

    let rx_mem = rx.recv().await.unwrap();
    assert_eq!(rx_mem.id, mem_n.id);

    // 3. Emit Agents island event
    let agent_n = NOTIFICATIONS
        .notify(
            IslandId::Agents,
            "Agent Task Completed",
            "Task completed for session sess-100 (agent-1) in 450ms",
            "info",
        )
        .await
        .expect("notify agents failed");
    assert_eq!(agent_n.island_id.as_str(), "agents");

    let rx_agent = rx.recv().await.unwrap();
    assert_eq!(rx_agent.id, agent_n.id);

    // 4. Emit Errors island event
    let err_n = NOTIFICATIONS
        .notify(
            IslandId::Errors,
            "Agent Execution Failed",
            "Error running agent for query: timeout",
            "error",
        )
        .await
        .expect("notify errors failed");
    assert_eq!(err_n.island_id.as_str(), "errors");

    let rx_err = rx.recv().await.unwrap();
    assert_eq!(rx_err.id, err_n.id);

    // Verify stored notifications in SQLite table
    let list = NOTIFICATIONS
        .list_notifications()
        .await
        .expect("list_notifications failed");

    assert!(list.iter().any(|n| n.id == sys_n.id && matches!(n.island_id, IslandId::System)));
    assert!(list.iter().any(|n| n.id == mem_n.id && matches!(n.island_id, IslandId::Memory)));
    assert!(list.iter().any(|n| n.id == agent_n.id && matches!(n.island_id, IslandId::Agents)));
    assert!(list.iter().any(|n| n.id == err_n.id && matches!(n.island_id, IslandId::Errors)));
}
