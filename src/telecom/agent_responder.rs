//! Autonomous agent participant in node chat rooms (Issue #2368 / WAVE-26.06)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum AgentResponderError {
    #[error("Trigger evaluation failed: {0}")]
    EvaluationFailed(String),

    #[error("Agent dispatch error: {0}")]
    DispatchError(String),

    #[error("Agent not bound to room {0}")]
    NotBound(String),

    #[error("Invalid trigger pattern: {0}")]
    InvalidPattern(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DispatchPolicy {
    AlwaysRespond,
    MentionOnly,
    KeywordMatch(Vec<String>),
    PrefixMatch(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TriggerPattern {
    Mention(String),
    Prefix(String),
    Regex(String),
    Keyword(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentParticipant {
    pub agent_id: String,
    pub display_name: String,
    pub model: String,
    pub dispatch_policy: DispatchPolicy,
    pub bound_rooms: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponsePayload {
    pub response_id: String,
    pub agent_id: String,
    pub room_id: String,
    pub replying_to_message_id: Option<String>,
    pub content: String,
    pub token_usage: u32,
    pub timestamp: DateTime<Utc>,
}

pub struct AgentResponderRegistry {
    agents: Arc<RwLock<HashMap<String, AgentParticipant>>>,
}

impl AgentResponderRegistry {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_agent(&self, agent: AgentParticipant) {
        let mut map = self.agents.write().await;
        map.insert(agent.agent_id.clone(), agent);
    }

    pub async fn bind_agent_to_room(
        &self,
        agent_id: &str,
        room_id: &str,
    ) -> Result<(), AgentResponderError> {
        let mut map = self.agents.write().await;
        if let Some(agent) = map.get_mut(agent_id) {
            if !agent.bound_rooms.contains(&room_id.to_string()) {
                agent.bound_rooms.push(room_id.to_string());
            }
            Ok(())
        } else {
            Err(AgentResponderError::NotBound(agent_id.to_string()))
        }
    }

    pub async fn get_agent(&self, agent_id: &str) -> Option<AgentParticipant> {
        let map = self.agents.read().await;
        map.get(agent_id).cloned()
    }
}

impl Default for AgentResponderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluates whether an incoming message triggers the agent's response policy.
pub fn evaluate_trigger(policy: &DispatchPolicy, agent_id: &str, message_text: &str) -> bool {
    match policy {
        DispatchPolicy::AlwaysRespond => true,
        DispatchPolicy::MentionOnly => {
            let mention_tag = format!("@{}", agent_id);
            message_text.contains(&mention_tag)
        }
        DispatchPolicy::KeywordMatch(keywords) => {
            let lower = message_text.to_lowercase();
            keywords.iter().any(|k| lower.contains(&k.to_lowercase()))
        }
        DispatchPolicy::PrefixMatch(prefix) => message_text.trim_start().starts_with(prefix),
    }
}

/// Generates an agent response payload for an incoming message.
pub fn generate_agent_reply(
    agent: &AgentParticipant,
    room_id: &str,
    incoming_message_id: Option<&str>,
    prompt: &str,
) -> Result<AgentResponsePayload, AgentResponderError> {
    if !agent.bound_rooms.contains(&room_id.to_string()) {
        return Err(AgentResponderError::NotBound(format!(
            "Agent {} not bound to room {}",
            agent.agent_id, room_id
        )));
    }

    let reply_content = format!(
        "Xavier Agent ACK: [{}] received query '{}'",
        agent.display_name, prompt
    );

    Ok(AgentResponsePayload {
        response_id: format!("resp_{}", Utc::now().timestamp_millis()),
        agent_id: agent.agent_id.clone(),
        room_id: room_id.to_string(),
        replying_to_message_id: incoming_message_id.map(|s| s.to_string()),
        content: reply_content,
        token_usage: prompt.split_whitespace().count() as u32 * 2 + 15,
        timestamp: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_trigger_policies() {
        let agent_id = "xavier-agent";

        assert!(evaluate_trigger(
            &DispatchPolicy::AlwaysRespond,
            agent_id,
            "hello there"
        ));

        assert!(evaluate_trigger(
            &DispatchPolicy::MentionOnly,
            agent_id,
            "hello @xavier-agent what is the status?"
        ));
        assert!(!evaluate_trigger(
            &DispatchPolicy::MentionOnly,
            agent_id,
            "hello xavier-agent without at"
        ));

        let kw_policy = DispatchPolicy::KeywordMatch(vec!["query".into(), "telemetry".into()]);
        assert!(evaluate_trigger(
            &kw_policy,
            agent_id,
            "Need telemetry info"
        ));
        assert!(!evaluate_trigger(&kw_policy, agent_id, "Just regular chat"));

        let prefix_policy = DispatchPolicy::PrefixMatch("!agent".into());
        assert!(evaluate_trigger(&prefix_policy, agent_id, "!agent status"));
        assert!(!evaluate_trigger(&prefix_policy, agent_id, "status !agent"));
    }

    #[tokio::test]
    async fn test_agent_registration_and_binding() {
        let registry = AgentResponderRegistry::new();
        let agent = AgentParticipant {
            agent_id: "sec-bot".into(),
            display_name: "Security Monitor".into(),
            model: "claude-3-5".into(),
            dispatch_policy: DispatchPolicy::AlwaysRespond,
            bound_rooms: vec!["room-1".into()],
            created_at: Utc::now(),
        };

        registry.register_agent(agent).await;
        let retrieved = registry.get_agent("sec-bot").await.unwrap();
        assert_eq!(retrieved.display_name, "Security Monitor");

        registry
            .bind_agent_to_room("sec-bot", "room-2")
            .await
            .expect("Binding room-2 should succeed");

        let updated = registry.get_agent("sec-bot").await.unwrap();
        assert!(updated.bound_rooms.contains(&"room-2".to_string()));
    }

    #[test]
    fn test_generate_agent_reply_flow() {
        let agent = AgentParticipant {
            agent_id: "analyst".into(),
            display_name: "Legal Analyst".into(),
            model: "gemini-pro".into(),
            dispatch_policy: DispatchPolicy::MentionOnly,
            bound_rooms: vec!["legal-room".into()],
            created_at: Utc::now(),
        };

        let res = generate_agent_reply(&agent, "legal-room", Some("msg-101"), "Review clause 4");
        assert!(res.is_ok());
        let payload = res.unwrap();
        assert_eq!(payload.agent_id, "analyst");
        assert_eq!(payload.replying_to_message_id, Some("msg-101".to_string()));
        assert!(payload.content.contains("Legal Analyst"));

        let err_res = generate_agent_reply(&agent, "unbound-room", None, "Review clause 4");
        assert!(err_res.is_err());
    }
}
