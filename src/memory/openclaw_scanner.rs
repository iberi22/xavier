// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! OpenClaw Agent Memory Scanner
//!
//! Scans the OpenClaw agents directory recursively, reading MEMORY.md, SOUL.md,
//! USER.md, TOOLS.md and daily log files for each discovered agent.
//!
//! The agents directory is resolved from:
//! 1. `XAVIER_AGENTS_DIR` environment variable
//! 2. Default: `C:\Users\<user>\clawd\agents` (Windows) or `~/clawd/agents` (Unix)

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

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

/// Scanner para encontrar agentes y su memoria en el sistema de archivos.
pub struct OpenClawAgentScanner {
    agents_dir: PathBuf,
}

impl Default for OpenClawAgentScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenClawAgentScanner {
    /// Crea un nuevo scanner. El directorio de agentes se resuelve desde:
    /// 1. La variable de entorno `XAVIER_AGENTS_DIR`
    /// 2. `%USERPROFILE%\clawd\agents` en Windows
    /// 3. `~/clawd/agents` como fallback universal
    pub fn new() -> Self {
        let agents_dir = Self::resolve_agents_dir();
        info!(
            "🔍 OpenClawAgentScanner initialized with agents_dir: {:?}",
            agents_dir
        );
        Self { agents_dir }
    }

    /// Crea un scanner apuntando a un directorio específico (útil para tests).
    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            agents_dir: path.as_ref().to_path_buf(),
        }
    }

    fn resolve_agents_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("XAVIER_AGENTS_DIR") {
            return PathBuf::from(dir);
        }

        // Try Windows %USERPROFILE%\clawd\agents
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let win_path = PathBuf::from(profile).join("clawd").join("agents");
            if win_path.exists() {
                return win_path;
            }
        }

        // Unix fallback: ~/clawd/agents
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("clawd").join("agents");
        }

        PathBuf::from("clawd/agents")
    }

    /// Escanea todos los agentes en `agents_dir` de forma recursiva.
    /// Un directorio es reconocido como agente si contiene un archivo `MEMORY.md`.
    pub async fn scan_all_agents(&self) -> Result<Vec<AgentMemory>> {
        info!(
            "🔍 Starting recursive OpenClaw Agent scan in {:?}",
            self.agents_dir
        );
        let mut agents = Vec::new();

        if !fs::try_exists(&self.agents_dir).await.unwrap_or(false) {
            debug!(
                "Agents directory {:?} does not exist, skipping scan",
                self.agents_dir
            );
            return Ok(agents);
        }

        // Iterative DFS to avoid stack overflow on deep trees
        let mut stack = vec![self.agents_dir.clone()];

        while let Some(current_path) = stack.pop() {
            let mut entries = match fs::read_dir(&current_path).await {
                Ok(e) => e,
                Err(e) => {
                    warn!("Could not read directory {:?}: {}", current_path, e);
                    continue;
                }
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let metadata = match entry.metadata().await {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("Could not read metadata for {:?}: {}", path, e);
                        continue;
                    }
                };

                if metadata.is_dir() {
                    let memory_md_path = path.join("MEMORY.md");
                    if fs::try_exists(&memory_md_path).await.unwrap_or(false) {
                        match self.scan_agent_dir(&path).await {
                            Ok(agent_memory) => agents.push(agent_memory),
                            Err(e) => warn!("Failed to scan agent directory {:?}: {}", path, e),
                        }
                        // Don't recurse further into an agent dir
                    } else {
                        // Continue recursive search into non-agent subdirs
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
        if !metadata.is_dir() {
            return Ok(None);
        }
        if !fs::try_exists(&agent_path.join("MEMORY.md"))
            .await
            .unwrap_or(false)
        {
            return Ok(None);
        }
        Ok(Some(self.scan_agent_dir(&agent_path).await?))
    }

    /// Lee y estructura todos los archivos de un directorio de agente.
    async fn scan_agent_dir(&self, path: &Path) -> Result<AgentMemory> {
        let agent_id = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Required: MEMORY.md
        let memory_md = fs::read_to_string(path.join("MEMORY.md")).await?;

        // Optional files
        let soul_md = Self::read_optional_file(&path.join("SOUL.md")).await;
        let user_md = Self::read_optional_file(&path.join("USER.md")).await;
        let tools_md = Self::read_optional_file(&path.join("TOOLS.md")).await;

        // Daily logs in memory/ subdirectory
        let daily_logs = Self::read_daily_logs(&path.join("memory")).await;

        debug!(
            "Scanned agent '{}': soul={}, user={}, tools={}, logs={}",
            agent_id,
            soul_md.is_some(),
            user_md.is_some(),
            tools_md.is_some(),
            daily_logs.len()
        );

        Ok(AgentMemory {
            agent_id,
            memory_md,
            soul_md,
            user_md,
            tools_md,
            daily_logs,
        })
    }

    async fn read_optional_file(path: &Path) -> Option<String> {
        if fs::try_exists(path).await.unwrap_or(false) {
            fs::read_to_string(path).await.ok()
        } else {
            None
        }
    }

    async fn read_daily_logs(logs_dir: &Path) -> Vec<DailyLog> {
        let mut logs = Vec::new();

        if !fs::try_exists(logs_dir).await.unwrap_or(false) {
            return logs;
        }

        let mut entries = match fs::read_dir(logs_dir).await {
            Ok(e) => e,
            Err(_) => return logs,
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let entry_path = entry.path();
            let is_md = entry_path.extension().and_then(|s| s.to_str()) == Some("md");
            if !is_md {
                continue;
            }

            // Extract date from filename, e.g. "2024-01-15.md" → "2024-01-15"
            let date = entry_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            if let Ok(content) = fs::read_to_string(&entry_path).await {
                logs.push(DailyLog { date, content });
            }
        }

        // Sort chronologically by date string
        logs.sort_by(|a, b| a.date.cmp(&b.date));
        logs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_scan_all_agents_empty_dir() -> Result<()> {
        let dir = tempdir()?;
        let scanner = OpenClawAgentScanner::with_dir(dir.path());
        let agents = scanner.scan_all_agents().await?;
        assert!(agents.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_scan_all_agents_finds_agents_with_memory_md() -> Result<()> {
        let dir = tempdir()?;
        let agents_root = dir.path();

        // Agent 1: complete with all files
        let a1 = agents_root.join("lasantacruz");
        fs::create_dir(&a1).await?;
        fs::write(a1.join("MEMORY.md"), "# Memory\n## Projects\nProyecto SWAL").await?;
        fs::write(a1.join("SOUL.md"), "Eres un agente...").await?;
        fs::write(a1.join("USER.md"), "El usuario es BELA").await?;
        fs::write(a1.join("TOOLS.md"), "## Tools\n- tool1").await?;
        let logs_dir = a1.join("memory");
        fs::create_dir(&logs_dir).await?;
        fs::write(logs_dir.join("2024-01-15.md"), "Log entry 1").await?;
        fs::write(logs_dir.join("2024-01-16.md"), "Log entry 2").await?;

        // Agent 2: only MEMORY.md
        let a2 = agents_root.join("xavier");
        fs::create_dir(&a2).await?;
        fs::write(a2.join("MEMORY.md"), "Xavier memory").await?;

        // Not an agent (no MEMORY.md)
        let not_agent = agents_root.join("random_dir");
        fs::create_dir(&not_agent).await?;
        fs::write(not_agent.join("README.md"), "Not an agent").await?;

        let scanner = OpenClawAgentScanner::with_dir(agents_root);
        let mut agents = scanner.scan_all_agents().await?;
        agents.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));

        assert_eq!(agents.len(), 2);

        let lsc = agents.iter().find(|a| a.agent_id == "lasantacruz").unwrap();
        assert!(lsc.memory_md.contains("Proyecto SWAL"));
        assert_eq!(lsc.soul_md.as_deref(), Some("Eres un agente..."));
        assert_eq!(lsc.user_md.as_deref(), Some("El usuario es BELA"));
        assert!(lsc.tools_md.is_some());
        assert_eq!(lsc.daily_logs.len(), 2);
        assert_eq!(lsc.daily_logs[0].date, "2024-01-15");
        assert_eq!(lsc.daily_logs[1].date, "2024-01-16");

        let xav = agents.iter().find(|a| a.agent_id == "xavier").unwrap();
        assert_eq!(xav.memory_md, "Xavier memory");
        assert!(xav.soul_md.is_none());
        assert!(xav.daily_logs.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_specific_agent() -> Result<()> {
        let dir = tempdir()?;
        let agents_root = dir.path();

        let agent_path = agents_root.join("my_agent");
        fs::create_dir(&agent_path).await?;
        fs::write(agent_path.join("MEMORY.md"), "my memory").await?;

        let scanner = OpenClawAgentScanner::with_dir(agents_root);

        let found = scanner.scan_agent("my_agent").await?;
        assert!(found.is_some());
        assert_eq!(found.unwrap().agent_id, "my_agent");

        let not_found = scanner.scan_agent("non_existent").await?;
        assert!(not_found.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_daily_logs_sorted_chronologically() -> Result<()> {
        let dir = tempdir()?;
        let agents_root = dir.path();

        let agent_path = agents_root.join("agent1");
        fs::create_dir(&agent_path).await?;
        fs::write(agent_path.join("MEMORY.md"), "memory").await?;

        let logs_dir = agent_path.join("memory");
        fs::create_dir(&logs_dir).await?;
        fs::write(logs_dir.join("2024-03-01.md"), "March log").await?;
        fs::write(logs_dir.join("2024-01-01.md"), "January log").await?;
        fs::write(logs_dir.join("2024-02-01.md"), "February log").await?;

        let scanner = OpenClawAgentScanner::with_dir(agents_root);
        let agents = scanner.scan_all_agents().await?;

        assert_eq!(agents.len(), 1);
        let logs = &agents[0].daily_logs;
        assert_eq!(logs[0].date, "2024-01-01");
        assert_eq!(logs[1].date, "2024-02-01");
        assert_eq!(logs[2].date, "2024-03-01");

        Ok(())
    }
}
