//! OpenClaw Agent Scanner
//!
//! Scans the OpenClaw agents directory to identify agents and their session files.

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Clone)]
pub struct AgentFile {
    pub path: PathBuf,
    pub size: u64,
    pub last_modified: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AgentScanResult {
    pub agent_id: String,
    pub files: Vec<AgentFile>,
    pub last_updated: DateTime<Utc>,
}

pub struct OpenClawAgentScanner {
    root_path: PathBuf,
}

impl OpenClawAgentScanner {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            root_path: path.as_ref().to_path_buf(),
        }
    }

    pub async fn scan_all_agents(&self) -> Result<Vec<AgentScanResult>> {
        let mut results = Vec::new();

        if !self.root_path.exists() {
            return Ok(results);
        }

        let mut entries = fs::read_dir(&self.root_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                if let Some(agent_id) = path.file_name().and_then(|n| n.to_str()) {
                    if let Ok(result) = self.scan_agent(agent_id).await {
                        results.push(result);
                    }
                }
            }
        }

        Ok(results)
    }

    pub async fn scan_agent(&self, agent_id: &str) -> Result<AgentScanResult> {
        let agent_path = self.root_path.join(agent_id);
        if !agent_path.exists() || !agent_path.is_dir() {
            anyhow::bail!("Agent directory not found: {}", agent_id);
        }

        let mut files = Vec::new();
        let mut last_updated = DateTime::<Utc>::MIN_UTC;

        let mut stack = vec![agent_path.clone()];
        while let Some(current_path) = stack.pop() {
            let mut entries = fs::read_dir(current_path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let metadata = entry.metadata().await?;

                if metadata.is_dir() {
                    stack.push(path);
                } else if metadata.is_file() {
                    let mtime: DateTime<Utc> = metadata.modified()?.into();
                    if mtime > last_updated {
                        last_updated = mtime;
                    }

                    files.push(AgentFile {
                        path,
                        size: metadata.len(),
                        last_modified: mtime,
                    });
                }
            }
        }

        if last_updated == DateTime::<Utc>::MIN_UTC {
            last_updated = Utc::now();
        }

        Ok(AgentScanResult {
            agent_id: agent_id.to_string(),
            files,
            last_updated,
        })
    }
}
