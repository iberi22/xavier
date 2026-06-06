use super::*;
use std::collections::HashMap;

#[tokio::test]
async fn test_register_unregister() {
    let bus = MessageBus::new();

    // Register agent
    let rx = bus
        .register_agent("agent1", "Test Agent", vec!["test".to_string()])
        .await;
    assert!(rx.is_ok());

    // Double registration should fail
    let rx = bus.register_agent("agent1", "Test Agent", vec![]).await;
    assert!(rx.is_err());

    // Unregister
    let result = bus.unregister_agent("agent1").await;
    assert!(result.is_ok());

    // Re-register should work
    let rx = bus.register_agent("agent1", "Test Agent", vec![]).await;
    assert!(rx.is_ok());
}

#[tokio::test]
async fn test_publish_subscribe() {
    let bus = MessageBus::new();

    // Register agents
    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");
    bus.register_agent("agent2", "Agent 2", vec![])
        .await
        .expect("test assertion");

    // Subscribe agent1 to topic
    bus.subscribe("agent1", "news")
        .await
        .expect("test assertion");

    // Publish to topic
    let msg = AgentMessage::new(
        "agent2",
        MessageType::Task,
        serde_json::json!({"text": "hello"}),
    )
    .on_topic("news");

    let count = bus.publish(msg).await.expect("test assertion");
    assert!(count >= 1);
}

#[tokio::test]
async fn test_direct_message() {
    let bus = MessageBus::new();

    bus.register_agent("sender", "Sender", vec![])
        .await
        .expect("test assertion");
    bus.register_agent("receiver", "Receiver", vec![])
        .await
        .expect("test assertion");

    let id = bus
        .send_direct("sender", "receiver", serde_json::json!({"text": "hello"}))
        .await
        .expect("test assertion");

    assert!(!id.is_empty());
}

#[tokio::test]
async fn test_broadcast() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");
    bus.register_agent("agent2", "Agent 2", vec![])
        .await
        .expect("test assertion");
    bus.register_agent("agent3", "Agent 3", vec![])
        .await
        .expect("test assertion");

    let id = bus
        .broadcast("sender", serde_json::json!({"text": "broadcast"}), None)
        .await
        .expect("test assertion");

    assert!(!id.is_empty());
}

#[tokio::test]
async fn test_request_response() {
    let bus = MessageBus::new();

    bus.register_agent("client", "Client", vec![])
        .await
        .expect("test assertion");
    bus.register_agent("server", "Server", vec![])
        .await
        .expect("test assertion");

    // This would need a server handler to respond
    // Just test the timeout case
    let result = bus
        .request(
            "client",
            "nonexistent",
            serde_json::json!({"test": "data"}),
            1,
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_metrics() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");

    let metrics = bus.get_metrics().await;
    assert_eq!(metrics.registered_agents, 1);
}

// Additional comprehensive tests

#[test]
fn test_message_priority_values() {
    assert_eq!(MessagePriority::Low.value(), 1);
    assert_eq!(MessagePriority::Normal.value(), 2);
    assert_eq!(MessagePriority::High.value(), 3);
    assert_eq!(MessagePriority::Critical.value(), 4);
}

#[test]
fn test_message_priority_default() {
    assert_eq!(MessagePriority::default(), MessagePriority::Normal);
}

#[test]
fn test_message_type_default() {
    assert_eq!(MessageType::default(), MessageType::Task);
}

#[test]
fn test_agent_message_creation() {
    let msg = AgentMessage::new(
        "sender1",
        MessageType::Task,
        serde_json::json!({"data": "test"}),
    );

    assert_eq!(msg.sender, "sender1");
    assert_eq!(msg.msg_type, MessageType::Task);
    assert_eq!(msg.retries, 0);
    assert_eq!(msg.max_retries, 3);
    assert!(!msg.id.is_empty());
}

#[test]
fn test_agent_message_task() {
    let msg = AgentMessage::task("sender1", serde_json::json!({"task": "do something"}));

    assert_eq!(msg.msg_type, MessageType::Task);
    assert_eq!(msg.sender, "sender1");
}

#[test]
fn test_agent_message_result() {
    let msg = AgentMessage::result("sender1", serde_json::json!({"result": "success"}));

    assert_eq!(msg.msg_type, MessageType::Result);
}

#[test]
fn test_agent_message_error() {
    let msg = AgentMessage::error("sender1", serde_json::json!({"error": "failed"}));

    assert_eq!(msg.msg_type, MessageType::Error);
}

#[test]
fn test_agent_message_heartbeat() {
    let msg = AgentMessage::heartbeat("sender1");

    assert_eq!(msg.msg_type, MessageType::Heartbeat);
    assert_eq!(msg.content["status"], "alive");
}

#[test]
fn test_agent_message_fluent_interface() {
    let msg = AgentMessage::task("sender1", serde_json::json!({}))
        .to("receiver1")
        .on_topic("test-topic")
        .with_correlation("corr-123")
        .reply_to_channel("reply-456")
        .with_priority(MessagePriority::High);

    assert_eq!(msg.receiver, Some("receiver1".to_string()));
    assert_eq!(msg.topic, Some("test-topic".to_string()));
    assert_eq!(msg.correlation_id, Some("corr-123".to_string()));
    assert_eq!(msg.reply_to, Some("reply-456".to_string()));
    assert_eq!(msg.priority, MessagePriority::High);
}

#[test]
fn test_agent_status_default() {
    assert_eq!(AgentStatus::default(), AgentStatus::Registered);
}

#[test]
fn test_agent_status_enum() {
    assert_eq!(AgentStatus::Registered as i32, 0);
    assert_eq!(AgentStatus::Active as i32, 1);
    assert_eq!(AgentStatus::Idle as i32, 2);
    assert_eq!(AgentStatus::Offline as i32, 3);
}

#[tokio::test]
async fn test_subscribe_nonexistent_agent() {
    let bus = MessageBus::new();

    let result = bus.subscribe("nonexistent", "topic").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_unsubscribe() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");
    bus.subscribe("agent1", "news")
        .await
        .expect("test assertion");

    let result = bus.unsubscribe("agent1", "news").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_heartbeat() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");

    let result = bus.heartbeat("agent1").await;
    assert!(result.is_ok());

    let agent = bus.get_agent("agent1").await;
    assert!(agent.is_some());
    assert_eq!(agent.expect("test assertion").status, AgentStatus::Active);
}

#[tokio::test]
async fn test_heartbeat_nonexistent_agent() {
    let bus = MessageBus::new();

    let result = bus.heartbeat("nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_agent() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Test Agent", vec!["capability1".to_string()])
        .await
        .expect("test assertion");

    let agent = bus.get_agent("agent1").await;
    assert!(agent.is_some());
    assert_eq!(agent.expect("test assertion").name, "Test Agent");
}

#[tokio::test]
async fn test_list_agents() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");
    bus.register_agent("agent2", "Agent 2", vec![])
        .await
        .expect("test assertion");

    let agents = bus.list_agents().await;
    assert_eq!(agents.len(), 2);
}

#[tokio::test]
async fn test_dlq_operations() {
    let bus = MessageBus::new();

    let msg = AgentMessage::task("sender", serde_json::json!({"test": "data"}));

    bus.send_to_dlq(msg.clone(), "test failure")
        .await
        .expect("test assertion");

    let dlq = bus.get_dlq().await;
    assert_eq!(dlq.len(), 1);

    let cleared = bus.clear_dlq().await;
    assert_eq!(cleared, 1);

    let dlq = bus.get_dlq().await;
    assert_eq!(dlq.len(), 0);
}

#[tokio::test]
async fn test_get_topics() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");
    bus.subscribe("agent1", "news")
        .await
        .expect("test assertion");
    bus.subscribe("agent1", "updates")
        .await
        .expect("test assertion");

    let topics = bus.get_topics().await;
    assert!(topics.contains_key("news"));
    assert!(topics.contains_key("updates"));
}

#[tokio::test]
async fn test_shutdown() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");
    bus.register_agent("agent2", "Agent 2", vec![])
        .await
        .expect("test assertion");

    bus.shutdown().await;

    let agents = bus.list_agents().await;
    assert_eq!(agents.len(), 0);
}

#[tokio::test]
async fn test_broadcast_with_topic() {
    let bus = MessageBus::new();

    bus.register_agent("agent1", "Agent 1", vec![])
        .await
        .expect("test assertion");
    bus.subscribe("agent1", "announcements")
        .await
        .expect("test assertion");

    let id = bus
        .broadcast(
            "sender",
            serde_json::json!({"text": "important"}),
            Some("announcements"),
        )
        .await
        .expect("test assertion");

    assert!(!id.is_empty());
}

#[test]
fn test_bus_metrics_serialization() {
    let metrics = BusMetrics {
        messages_sent: 100,
        messages_received: 95,
        messages_failed: 5,
        dlq_size: 2,
        registered_agents: 3,
        topic_subscriptions: HashMap::new(),
        queue_sizes: HashMap::new(),
        stale_agents: 0,
        heartbeats_received: 0,
    };

    let json = serde_json::to_string(&metrics).expect("test assertion");
    assert!(json.contains("100"));
    assert!(json.contains("95"));
}

#[test]
fn test_message_bus_error_display() {
    let err = MessageBusError::AgentNotFound("agent1".to_string());
    assert!(err.to_string().contains("agent1"));

    let err = MessageBusError::RequestTimeout(5);
    assert!(err.to_string().contains("5"));

    let err = MessageBusError::ChannelClosed;
    assert!(err.to_string().contains("Channel closed"));
}
