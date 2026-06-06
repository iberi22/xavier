/// Message Bus errors
#[derive(Debug, thiserror::Error)]
pub enum MessageBusError {
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Agent already registered: {0}")]
    AgentAlreadyRegistered(String),

    #[error("Request timeout after {0} seconds")]
    RequestTimeout(u64),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Message send failed: {0}")]
    SendFailed(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

impl serde::Serialize for MessageBusError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
