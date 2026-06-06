use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// Message priority levels
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessagePriority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

impl MessagePriority {
    pub fn value(&self) -> u8 {
        match self {
            MessagePriority::Low => 1,
            MessagePriority::Normal => 2,
            MessagePriority::High => 3,
            MessagePriority::Critical => 4,
        }
    }
}

/// Message types for agent communication
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    #[default]
    Task,
    Result,
    Error,
    Heartbeat,
    Register,
    Unregister,
    Shutdown,
}

/// Core message structure for agent communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Unique message ID
    pub id: String,

    /// Sender agent ID
    pub sender: String,

    /// Receiver agent ID (None = broadcast/topic)
    pub receiver: Option<String>,

    /// Topic for pub/sub (None = direct message)
    pub topic: Option<String>,

    /// Message type
    pub msg_type: MessageType,

    /// Message content (any serializable data)
    pub content: serde_json::Value,

    /// Priority level
    pub priority: MessagePriority,

    /// Timestamp
    pub timestamp: DateTime<Utc>,

    /// Correlation ID for request/response
    pub correlation_id: Option<String>,

    /// Reply channel ID
    pub reply_to: Option<String>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,

    /// Retry count
    pub retries: u32,

    /// Max retries before going to DLQ
    pub max_retries: u32,
}

impl AgentMessage {
    /// Create a new message
    pub fn new(sender: &str, msg_type: MessageType, content: serde_json::Value) -> Self {
        Self {
            id: Ulid::new().to_string(),
            sender: sender.to_string(),
            receiver: None,
            topic: None,
            msg_type,
            content,
            priority: MessagePriority::default(),
            timestamp: Utc::now(),
            correlation_id: None,
            reply_to: None,
            metadata: HashMap::new(),
            retries: 0,
            max_retries: 3,
        }
    }

    /// Create a task message
    pub fn task(sender: &str, content: serde_json::Value) -> Self {
        Self::new(sender, MessageType::Task, content)
    }

    /// Create a result message
    pub fn result(sender: &str, content: serde_json::Value) -> Self {
        Self::new(sender, MessageType::Result, content)
    }

    /// Create an error message
    pub fn error(sender: &str, content: serde_json::Value) -> Self {
        Self::new(sender, MessageType::Error, content)
    }

    /// Create a heartbeat message
    pub fn heartbeat(sender: &str) -> Self {
        Self::new(
            sender,
            MessageType::Heartbeat,
            serde_json::json!({ "status": "alive" }),
        )
    }

    /// Set receiver (direct message)
    pub fn to(mut self, receiver: &str) -> Self {
        self.receiver = Some(receiver.to_string());
        self
    }

    /// Set topic (pub/sub)
    pub fn on_topic(mut self, topic: &str) -> Self {
        self.topic = Some(topic.to_string());
        self
    }

    /// Set correlation ID for request/response
    pub fn with_correlation(mut self, correlation_id: &str) -> Self {
        self.correlation_id = Some(correlation_id.to_string());
        self
    }

    /// Set reply channel
    pub fn reply_to_channel(mut self, channel_id: &str) -> Self {
        self.reply_to = Some(channel_id.to_string());
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: MessagePriority) -> Self {
        self.priority = priority;
        self
    }
}

/// Agent subscription info
#[derive(Debug, Clone)]
pub struct Subscription {
    pub agent_id: String,
    pub topic: String,
}

/// Result of a request/response operation
#[derive(Debug)]
pub struct Response {
    pub message: AgentMessage,
    pub received_at: DateTime<Utc>,
}

/// Heartbeat configuration
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Timeout in seconds before marking agent as offline
    pub timeout_secs: u64,
    /// Interval to check for stale heartbeats
    pub check_interval_secs: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            check_interval_secs: 10,
        }
    }
}

/// Agent information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<String>,
    pub registered_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub status: AgentStatus,
}

/// Agent status
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    #[default]
    Registered,
    Active,
    Idle,
    Offline,
}
