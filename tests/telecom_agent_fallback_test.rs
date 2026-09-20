use chrono::Utc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use xavier::telecom::agent_responder::{
    AgentParticipant, AgentResponsePayload, DispatchPolicy,
};

// Mock function that simulates a slow agent response (taking longer than 5000ms)
async fn slow_agent_dispatch(
    agent: &AgentParticipant,
    _room_id: &str,
    _incoming_msg_id: &str,
    prompt: &str,
) -> Result<AgentResponsePayload, String> {
    // Simulate a delay of 6000ms (longer than 5000ms timeout)
    sleep(Duration::from_millis(6000)).await;

    // Normal reply (should not be reached due to timeout)
    let reply_content = format!(
        "Xavier Agent ACK: [{}] received query '{}'",
        agent.display_name, prompt
    );

    Ok(AgentResponsePayload {
        response_id: format!("resp_{}", Utc::now().timestamp_millis()),
        agent_id: agent.agent_id.clone(),
        room_id: _room_id.to_string(),
        replying_to_message_id: Some(_incoming_msg_id.to_string()),
        content: reply_content,
        token_usage: 42,
        timestamp: Utc::now(),
    })
}

// The dispatcher wrapper implementing the fallback logic
async fn dispatch_with_fallback(
    agent: &AgentParticipant,
    room_id: &str,
    incoming_msg_id: &str,
    prompt: &str,
) -> AgentResponsePayload {
    let result = timeout(
        Duration::from_millis(5000),
        slow_agent_dispatch(agent, room_id, incoming_msg_id, prompt),
    )
    .await;

    match result {
        Ok(Ok(payload)) => payload,
        Ok(Err(e)) => AgentResponsePayload {
            response_id: format!("resp_err_{}", Utc::now().timestamp_millis()),
            agent_id: agent.agent_id.clone(),
            room_id: room_id.to_string(),
            replying_to_message_id: Some(incoming_msg_id.to_string()),
            content: format!("Error: {}", e),
            token_usage: 0,
            timestamp: Utc::now(),
        },
        Err(_) => AgentResponsePayload {
            response_id: format!("resp_err_{}", Utc::now().timestamp_millis()),
            agent_id: agent.agent_id.clone(),
            room_id: room_id.to_string(),
            replying_to_message_id: Some(incoming_msg_id.to_string()),
            content: "Error: Agent auto-responder is offline or taking too long to respond (>5000ms). Falling back.".to_string(),
            token_usage: 0,
            timestamp: Utc::now(),
        },
    }
}

#[tokio::test]
async fn test_telecom_agent_fallback_on_timeout() {
    let agent = AgentParticipant {
        agent_id: "auto-responder-1".to_string(),
        display_name: "Auto Responder".to_string(),
        model: "mock-model".to_string(),
        dispatch_policy: DispatchPolicy::AlwaysRespond,
        bound_rooms: vec!["test-room-123".to_string()],
        created_at: Utc::now(),
    };

    let room_id = "test-room-123";
    let incoming_msg_id = "msg-001";
    let prompt = "Urgent request";

    // Dispatch the request using the wrapper that enforces the timeout and fallback
    let fallback_error_payload = dispatch_with_fallback(&agent, room_id, incoming_msg_id, prompt).await;

    // Assert the fallback payload structure
    assert_eq!(fallback_error_payload.agent_id, "auto-responder-1");
    assert_eq!(fallback_error_payload.room_id, "test-room-123");
    assert_eq!(fallback_error_payload.replying_to_message_id.unwrap(), "msg-001");
    assert!(fallback_error_payload.content.contains(">5000ms"));
    assert!(fallback_error_payload.content.contains("offline"));
}
