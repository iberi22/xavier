use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::codebase::conversations_db::{Message, ThreadSummary};

#[derive(Debug, Deserialize)]
pub struct CreateThreadRequest {
    pub title: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PanelChatRequest {
    pub thread_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PanelChatResponse {
    pub thread: ThreadSummary,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub url: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    #[serde(rename = "type")]
    pub widget_type: String,
    pub config: serde_json::Value,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub id: String,
    pub name: String,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}
