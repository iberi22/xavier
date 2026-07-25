//! Agent memory scanner
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// Representa una sesión de chat extraída de un IDE/Agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub ide: String,
    pub project_path: Option<String>,
    pub updated_at: String,
    pub messages: Vec<AgentMessage>,
    pub source_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone)]
pub struct AgentScanner {
    // Rutas base a escanear (e.g., ~/.cursor, %APPDATA%/Cursor)
    search_paths: Vec<PathBuf>,
}

impl Default for AgentScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentScanner {
    /// New.
    pub fn new() -> Self {
        let mut paths = Vec::new();

        // Obtener directorios home y appdata
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".cursor"));
            paths.push(home.join(".windsurf"));
            paths.push(home.join(".vscode"));
        }

        if let Some(data_local) = dirs::data_local_dir() {
            paths.push(data_local.join("Cursor"));
            paths.push(data_local.join("Windsurf"));
        }

        if let Some(config) = dirs::config_dir() {
            // AppData/Roaming on Windows
            paths.push(config.join("Cursor"));
            paths.push(config.join("Windsurf"));
            paths.push(config.join("Code"));
            paths.push(config.join("Kiro"));
        }

        Self {
            search_paths: paths.into_iter().filter(|p| p.exists()).collect(),
        }
    }

    /// Scan all.
    pub async fn scan_all(&self) -> Result<Vec<AgentSession>> {
        let mut all_sessions = Vec::new();
        info!("🔍 Starting system-wide Agent IDE scan...");

        for path in &self.search_paths {
            debug!("Scanning base directory: {:?}", path);
            match self.scan_directory(path).await {
                Ok(sessions) => {
                    all_sessions.extend(sessions);
                }
                Err(e) => {
                    warn!("Failed to scan directory {:?}: {}", path, e);
                }
            }
        }

        info!(
            "✅ Found {} agent conversation sessions.",
            all_sessions.len()
        );
        Ok(all_sessions)
    }

    async fn scan_directory(&self, root: &Path) -> Result<Vec<AgentSession>> {
        let mut sessions = Vec::new();
        let mut stack = vec![root.to_path_buf()];

        while let Some(path) = stack.pop() {
            if !path.is_dir() {
                continue;
            }

            if let Ok(mut entries) = fs::read_dir(&path).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let entry_path = entry.path();

                    if entry_path.is_dir() {
                        stack.push(entry_path);
                    } else if entry_path.is_file() {
                        // Heuristic extraction based on extension
                        if let Some(ext) = entry_path.extension().and_then(|e| e.to_str()) {
                            match ext {
                                "vscdb" | "sqlite" | "db" => {
                                    if let Ok(Some(session)) =
                                        self.extract_from_sqlite(&entry_path).await
                                    {
                                        sessions.push(session);
                                    }
                                }
                                "json" | "jsonl" => {
                                    if let Ok(Some(session)) =
                                        self.extract_from_json(&entry_path).await
                                    {
                                        sessions.push(session);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        Ok(sessions)
    }

    /// Extracción genérica de SQLite (e.g. Cursor state.vscdb)
    async fn extract_from_sqlite(&self, path: &Path) -> Result<Option<AgentSession>> {
        let path_clone = path.to_path_buf();
        // Run sqlite blocking IO in spawn_blocking to not block the tokio reactor
        let result = tokio::task::spawn_blocking(move || {
            let conn = Connection::open_with_flags(
                &path_clone,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            )
            .ok()?;

            // Attempt to read from common vscode/cursor key-value tables
            // This is a generic heuristic: look for keys containing 'chat' or 'cursor'
            let mut stmt = conn.prepare("SELECT key, value FROM ItemTable").ok()?;

            let mut rows = stmt.query([]).ok()?;
            let mut messages = Vec::new();
            let updated_at = chrono::Utc::now().to_rfc3339();

            while let Some(row) = rows.next().ok().flatten() {
                let key: String = row.get(0).ok()?;
                let value: String = row.get(1).ok()?;

                if key.to_lowercase().contains("chat") || key.to_lowercase().contains("composer") {
                    // Very naive heuristic to find dialogue
                    if value.contains("\"role\"") && value.contains("\"content\"") {
                        messages.push(AgentMessage {
                            role: "extracted_data".to_string(),
                            content: format!(
                                "Found fragment in {}: {}",
                                key,
                                crate::memory::snippet::clip_chars(&value, 2000)
                            ),
                        });
                    }
                }
            }

            if !messages.is_empty() {
                Some(AgentSession {
                    ide: "Unknown_IDE".to_string(),
                    project_path: None,
                    updated_at,
                    messages,
                    source_file: path_clone.to_string_lossy().to_string(),
                })
            } else {
                None
            }
        })
        .await
        .unwrap_or(None);

        Ok(result)
    }

    /// Extracción genérica de JSON (e.g. Copilot chat history)
    async fn extract_from_json(&self, path: &Path) -> Result<Option<AgentSession>> {
        let content = fs::read_to_string(path).await?;

        // Naive heuristic for JSON that looks like chat history
        if content.contains("\"role\"")
            && content.contains("\"content\"")
            && content.contains("\"user\"")
        {
            // Attempt generic parsing
            let messages = vec![AgentMessage {
                role: "raw_json_extract".to_string(),
                content: crate::memory::snippet::clip_chars(&content, 3000).to_string(), // Limit size
            }];

            Ok(Some(AgentSession {
                ide: "JSON_Agent".to_string(),
                project_path: None,
                updated_at: chrono::Utc::now().to_rfc3339(),
                messages,
                source_file: path.to_string_lossy().to_string(),
            }))
        } else {
            Ok(None)
        }
    }
}
