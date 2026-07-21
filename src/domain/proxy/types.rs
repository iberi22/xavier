// SPDX-License-Identifier: MIT OR LICENSE-MESH
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
    Gemini,
    Groq,
    Zai,
    OpenCode,
    Generic,
}

impl ProviderKind {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" => Self::OpenAI,
            "anthropic" => Self::Anthropic,
            "gemini" | "google" => Self::Gemini,
            "groq" => Self::Groq,
            "z.ai" | "zai" => Self::Zai,
            "opencode" => Self::OpenCode,
            _ => Self::Generic,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Groq => "groq",
            Self::Zai => "z.ai",
            Self::OpenCode => "opencode",
            Self::Generic => "generic",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderQuota {
    pub provider: ProviderKind,
    pub api_tier: ApiTier,
    pub requests_remaining: Option<u64>,
    pub tokens_remaining: Option<u64>,
    pub requests_limit: Option<u64>,
    pub tokens_limit: Option<u64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub last_checked: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApiTier {
    Free,
    Pro,
    Enterprise,
    Unknown,
}

impl ApiTier {
    pub fn from_rpm(rpm: u64) -> Self {
        if rpm == 0 {
            Self::Unknown
        } else if rpm < 60 {
            Self::Free
        } else if rpm < 500 {
            Self::Pro
        } else {
            Self::Enterprise
        }
    }
}
