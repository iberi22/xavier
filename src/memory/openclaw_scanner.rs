//! OpenClaw Agent memory scanner
//!
//! Scans OpenClaw agent directories for memory files (MEMORY.md, TOOLS.md, etc.)
//! and daily log files.

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info};
use chrono::{DateTime, Utc};

/// Scanner for OpenClaw agents located in a specific directory.
pub struct OpenClawAgentScanner {
    agents_dir: PathBuf,
}

/// Representa el conjunto de archivos de memoria de un agente OpenClaw.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    pub agent_id: String,
    pub memory_md: AgentFile,
    pub tools_md: Option<AgentFile>,
    pub soul_md: Option<AgentFile>,
    pub user_md: Option<AgentFile>,
    pub daily_logs: Vec<AgentFile>,
}

/// Representa un archivo individual de la memoria del agente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentFile {
    pub path: String,
    pub content: String,
    pub modified_at: String,
}

/// Representa un log diario (opcionalmente usado para procesamiento posterior).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyLog {
    pub date: String,
    pub content: String,
}

impl OpenClawAgentScanner {
    /// Crea un nuevo scanner apuntando al directorio de agentes.
    pub fn new(agents_dir: &str) -> Self {
        Self {
            agents_dir: PathBuf::from(agents_dir),
        }
    }

    /// Escanea TODOS los subdirectorios de agents/ recursivamente buscando MEMORY.md
    pub async fn scan_all_agents(&self) -> Result<Vec<AgentMemory>> {
        info!("🔍 Starting recursive OpenClaw Agent scan in {:?}", self.agents_dir);
        let mut agents = Vec::new();

        if !fs::try_exists(&self.agents_dir).await.unwrap_or(false) {
            debug!("Agents directory {:?} does not exist", self.agents_dir);
            return Ok(agents);
        }

        let mut stack = vec![self.agents_dir.clone()];

        while let Some(current_path) = stack.pop() {
            let mut entries = fs::read_dir(&current_path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let metadata = entry.metadata().await?;

                if metadata.is_dir() {
                    let memory_md_path = path.join("MEMORY.md");
                    if fs::try_exists(&memory_md_path).await.unwrap_or(false) {
                        match self.scan_agent_dir(&path).await {
                            Ok(agent_memory) => agents.push(agent_memory),
                            Err(e) => debug!("Failed to scan agent directory {:?}: {}", path, e),
                        }
                    } else {
                        // Continue recursive search
                        stack.push(path);
                    }
                }
            }
        }

        info!("✅ Found {} OpenClaw agents.", agents.len());
        Ok(agents)
    }

    /// Escanea un agente específico por nombre.
    pub async fn scan_agent(&self, name: &str) -> Result<Option<AgentMemory>> {
        let agent_path = self.agents_dir.join(name);
        if !fs::try_exists(&agent_path).await.unwrap_or(false) {
            return Ok(None);
        }

        let metadata = fs::metadata(&agent_path).await?;
        if !metadata.is_dir() || !fs::try_exists(&agent_path.join("MEMORY.md")).await.unwrap_or(false) {
            return Ok(None);
        }

        Ok(Some(self.scan_agent_dir(&agent_path).await?))
    }

    /// Lee archivos de un directorio de agente específico.
    pub async fn scan_agent_dir(&self, path: &Path) -> Result<AgentMemory> {
        let agent_id = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let memory_md_path = path.join("MEMORY.md");
        let memory_md = self.read_agent_file(&memory_md_path).await?
            .context("MEMORY.md is required for an OpenClaw agent")?;

        let tools_md = self.read_agent_file(&path.join("TOOLS.md")).await?;
        let soul_md = self.read_agent_file(&path.join("SOUL.md")).await?;
        let user_md = self.read_agent_file(&path.join("USER.md")).await?;

        let mut daily_logs = Vec::new();
        let logs_dir = path.join("memory");

        if fs::try_exists(&logs_dir).await.unwrap_or(false) {
            let logs_metadata = fs::metadata(&logs_dir).await?;
            if logs_metadata.is_dir() {
                let mut entries = fs::read_dir(logs_dir).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let entry_path = entry.path();
                    let entry_metadata = entry.metadata().await?;
                    if entry_metadata.is_file() && entry_path.extension().and_then(|s| s.to_str()) == Some("md") {
                        if let Some(file) = self.read_agent_file(&entry_path).await? {
                            daily_logs.push(file);
                        }
                    }
                }
            }
        }

        Ok(AgentMemory {
            agent_id,
            memory_md,
            tools_md,
            soul_md,
            user_md,
            daily_logs,
        })
    }

    async fn read_agent_file(&self, path: &Path) -> Result<Option<AgentFile>> {
        if !fs::try_exists(path).await.unwrap_or(false) {
            return Ok(None);
        }

        let metadata = fs::metadata(path).await?;
        if !metadata.is_file() {
            return Ok(None);
        }

        let modified: DateTime<Utc> = metadata.modified()?.into();
        let content = fs::read_to_string(path).await?;

        Ok(Some(AgentFile {
            path: path.to_string_lossy().to_string(),
            content,
            modified_at: modified.to_rfc3339(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_scan_all_agents() -> Result<()> {
        let dir = tempdir()?;
        let agents_root = dir.path();

        // Agent 1: complete
        let agent1_path = agents_root.join("agent1");
        fs::create_dir(&agent1_path).await?;
        fs::write(agent1_path.join("MEMORY.md"), "agent1 memory").await?;
        fs::write(agent1_path.join("SOUL.md"), "agent1 soul").await?;

        let agent1_logs = agent1_path.join("memory");
        fs::create_dir(&agent1_logs).await?;
        fs::write(agent1_logs.join("2024-01-01.md"), "log 1").await?;

        // Agent 2: minimal
        let agent2_path = agents_root.join("agent2");
        fs::create_dir(&agent2_path).await?;
        fs::write(agent2_path.join("MEMORY.md"), "agent2 memory").await?;

        // Agent 3: nested
        let nested_dir = agents_root.join("nested");
        fs::create_dir(&nested_dir).await?;
        let agent3_path = nested_dir.join("agent3");
        fs::create_dir(&agent3_path).await?;
        fs::write(agent3_path.join("MEMORY.md"), "agent3 memory").await?;

        // Not an agent (missing MEMORY.md)
        let not_agent_path = agents_root.join("not_agent");
        fs::create_dir(&not_agent_path).await?;
        fs::write(not_agent_path.join("README.md"), "just a readme").await?;

        let scanner = OpenClawAgentScanner::new(agents_root.to_str().unwrap());
        let agents = scanner.scan_all_agents().await?;

        assert_eq!(agents.len(), 3);

        let a1 = agents.iter().find(|a| a.agent_id == "agent1").unwrap();
        assert_eq!(a1.memory_md.content, "agent1 memory");
        assert!(a1.soul_md.is_some());
        assert_eq!(a1.soul_md.as_ref().unwrap().content, "agent1 soul");
        assert_eq!(a1.daily_logs.len(), 1);
        assert_eq!(a1.daily_logs[0].content, "log 1");

        let a2 = agents.iter().find(|a| a.agent_id == "agent2").unwrap();
        assert_eq!(a2.memory_md.content, "agent2 memory");
        assert!(a2.soul_md.is_none());
        assert!(a2.daily_logs.is_empty());

        let a3 = agents.iter().find(|a| a.agent_id == "agent3").unwrap();
        assert_eq!(a3.memory_md.content, "agent3 memory");

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_specific_agent() -> Result<()> {
        let dir = tempdir()?;
        let agents_root = dir.path();

        let agent_path = agents_root.join("my_agent");
        fs::create_dir(&agent_path).await?;
        fs::write(agent_path.join("MEMORY.md"), "my memory").await?;

        let scanner = OpenClawAgentScanner::new(agents_root.to_str().unwrap());

        let found = scanner.scan_agent("my_agent").await?;
        assert!(found.is_some());
        assert_eq!(found.unwrap().agent_id, "my_agent");

        let not_found = scanner.scan_agent("non_existent").await?;
        assert!(not_found.is_none());

        Ok(())
    }
}
