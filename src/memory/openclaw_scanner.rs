//! OpenClaw Agent Memory Scanner
//!
//! Provides structures and logic to scan OpenClaw agent workspaces for memory files.

use serde::{Deserialize, Serialize};
use anyhow::Result;

/// Representa un log diario de un agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyLog {
    pub date: String,
    pub content: String,
}

/// Representa la memoria completa de un agente de OpenClaw
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    pub agent_id: String,
    pub memory_md: String,
    pub soul_md: Option<String>,
    pub user_md: Option<String>,
    pub tools_md: Option<String>,
    pub daily_logs: Vec<DailyLog>,
}

/// Scanner para encontrar agentes y su memoria en el sistema
pub struct OpenClawAgentScanner {
}

impl Default for OpenClawAgentScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenClawAgentScanner {
    pub fn new() -> Self {
        Self {}
    }

    /// Escanea todos los agentes disponibles (mock implementation)
    pub async fn scan_all_agents(&self) -> Result<Vec<AgentMemory>> {
        // En una implementación real, esto buscaría en directorios como ~/clawd/agents/
        Ok(Vec::new())
    }
}
