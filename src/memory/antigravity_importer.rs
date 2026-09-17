//! Google Antigravity Session Importer
//!
//! Scans ~/.gemini/antigravity/brain/ (or `ANTIGRAVITY_BRAIN_DIR` env var) for session
//! transcript JSONL files and indexes them into Xavier `MemoryStore` under `antigravity://` paths.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::kernel::filters::strip_ansi;
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

/// A single turn in an Antigravity conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityTurn {
    pub role: String,
    pub content: String,
    pub tool_calls: Vec<String>,
}

/// A parsed Antigravity session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravitySession {
    pub session_id: String,
    pub transcript_path: PathBuf,
    pub turns: Vec<AntigravityTurn>,
}

pub struct AntigravityImporter {
    brain_dir: PathBuf,
    embedder: Option<Arc<dyn Embedder>>,
}

impl Default for AntigravityImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl AntigravityImporter {
    pub fn new() -> Self {
        let brain_dir = Self::resolve_brain_dir();
        Self {
            brain_dir,
            embedder: None,
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            brain_dir: path.as_ref().to_path_buf(),
            embedder: None,
        }
    }

    pub fn with_dir_and_embedder<P: AsRef<Path>>(path: P, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            brain_dir: path.as_ref().to_path_buf(),
            embedder: Some(embedder),
        }
    }

    fn resolve_brain_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("ANTIGRAVITY_BRAIN_DIR") {
            return PathBuf::from(dir);
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home)
                .join(".gemini")
                .join("antigravity")
                .join("brain");
            if path.exists() {
                return path;
            }
        }
        PathBuf::from(".gemini/antigravity/brain")
    }

    /// Scan `brain_dir` for session directories containing transcript.jsonl.
    pub async fn scan_sessions(&self) -> Result<Vec<AntigravitySession>> {
        let mut sessions = Vec::new();

        if !self.brain_dir.exists() {
            debug!(
                "Antigravity brain dir {:?} does not exist. Skipping.",
                self.brain_dir
            );
            return Ok(sessions);
        }

        let mut read_dir = match fs::read_dir(&self.brain_dir).await {
            Ok(rd) => rd,
            Err(e) => {
                warn!("Failed to read Antigravity brain dir: {}", e);
                return Ok(sessions);
            }
        };

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                let session_id = entry.file_name().to_string_lossy().to_string();

                // Check .system_generated/logs/transcript.jsonl
                let primary_transcript = path
                    .join(".system_generated")
                    .join("logs")
                    .join("transcript.jsonl");
                let fallback_transcript = path.join("transcript.jsonl");

                let transcript_path = if primary_transcript.exists() {
                    Some(primary_transcript)
                } else if fallback_transcript.exists() {
                    Some(fallback_transcript)
                } else {
                    None
                };

                if let Some(tpath) = transcript_path {
                    match self.parse_transcript(&session_id, &tpath).await {
                        Ok(session) => {
                            if !session.turns.is_empty() {
                                sessions.push(session);
                            }
                        }
                        Err(e) => {
                            debug!("Failed to parse Antigravity transcript {:?}: {}", tpath, e);
                        }
                    }
                }
            }
        }

        info!(
            "🔍 AntigravityImporter found {} valid sessions in {:?}",
            sessions.len(),
            self.brain_dir
        );
        Ok(sessions)
    }

    /// Parse a JSONL transcript file into an `AntigravitySession`.
    pub async fn parse_transcript(
        &self,
        session_id: &str,
        path: &Path,
    ) -> Result<AntigravitySession> {
        let content = fs::read_to_string(path).await?;
        let mut turns = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(val) => val,
                Err(_) => continue,
            };

            let step_type = v["type"].as_str().unwrap_or("");

            if step_type == "USER_INPUT" {
                if let Some(raw_content) = v["content"].as_str() {
                    let cleaned = strip_ansi(raw_content).trim().to_string();
                    if !cleaned.is_empty() {
                        turns.push(AntigravityTurn {
                            role: "user".to_string(),
                            content: cleaned,
                            tool_calls: Vec::new(),
                        });
                    }
                }
            } else if step_type == "PLANNER_RESPONSE" {
                let mut tool_names = Vec::new();
                if let Some(tool_calls) = v["tool_calls"].as_array() {
                    for tc in tool_calls {
                        if let Some(name) = tc["name"].as_str() {
                            let summary = tc["arguments"]["toolSummary"]
                                .as_str()
                                .or_else(|| tc["args"]["toolSummary"].as_str())
                                .unwrap_or("");
                            if summary.is_empty() {
                                tool_names.push(name.to_string());
                            } else {
                                tool_names.push(format!("{}({})", name, summary));
                            }
                        }
                    }
                }

                let response_text = v["content"]
                    .as_str()
                    .map(|s| strip_ansi(s).trim().to_string())
                    .unwrap_or_default();

                if !response_text.is_empty() || !tool_names.is_empty() {
                    turns.push(AntigravityTurn {
                        role: "assistant".to_string(),
                        content: response_text,
                        tool_calls: tool_names,
                    });
                }
            }
        }

        Ok(AntigravitySession {
            session_id: session_id.to_string(),
            transcript_path: path.to_path_buf(),
            turns,
        })
    }

    /// Import a single parsed Antigravity session into Xavier `MemoryStore`.
    pub async fn import_session(
        &self,
        session: &AntigravitySession,
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        let mut records = Vec::new();
        let path = format!("antigravity://sessions/{}", session.session_id);
        let workspace_id = "agent:antigravity".to_string();

        let mut full_text = format!("# Google Antigravity Session: {}\n\n", session.session_id);

        for turn in &session.turns {
            full_text.push_str(&format!("### [{}]\n", turn.role));
            if !turn.tool_calls.is_empty() {
                full_text.push_str(&format!("🔧 **Tools**: {}\n\n", turn.tool_calls.join(", ")));
            }
            if !turn.content.is_empty() {
                full_text.push_str(&format!("{}\n\n", turn.content));
            }
        }

        let mut record = MemoryRecord {
            workspace_id: workspace_id.clone(),
            path: path.clone(),
            content: full_text,
            metadata: json!({
                "source_app": "antigravity",
                "agent_id": "antigravity",
                "session_id": session.session_id,
                "transcript_path": session.transcript_path.to_string_lossy(),
                "turn_count": session.turns.len(),
                "dedup": true,
            }),
            ..Default::default()
        };

        record.id = stable_key("memory", &[&workspace_id, &path]);

        if let Some(embedder) = &self.embedder {
            if let Ok(emb) = embedder.encode(&record.content).await {
                record.embedding = emb;
            }
        }

        store.put(record.clone()).await?;
        records.push(record);

        Ok(records)
    }

    /// Scan and import all Antigravity sessions into the given `MemoryStore`.
    pub async fn import_all(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        let sessions = self.scan_sessions().await?;
        let mut imported = Vec::new();

        for s in sessions {
            match self.import_session(&s, store).await {
                Ok(recs) => imported.extend(recs),
                Err(e) => warn!(
                    "Failed to import Antigravity session {}: {}",
                    s.session_id, e
                ),
            }
        }

        info!(
            "✅ Successfully imported {} Antigravity session records",
            imported.len()
        );
        Ok(imported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::InMemoryMemoryStore;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_parse_and_import_antigravity_transcript() -> Result<()> {
        let dir = tempdir()?;
        let session_dir = dir.path().join("sess_test_123");
        let logs_dir = session_dir.join(".system_generated").join("logs");
        fs::create_dir_all(&logs_dir).await?;

        let transcript_file = logs_dir.join("transcript.jsonl");
        let lines = vec![
            json!({
                "step_index": 0,
                "type": "USER_INPUT",
                "content": "\x1B[31mHello Antigravity!\x1B[0m",
            }),
            json!({
                "step_index": 1,
                "type": "PLANNER_RESPONSE",
                "content": "I am ready to assist you.",
                "tool_calls": [
                    {
                        "name": "view_file",
                        "arguments": { "toolSummary": "Read config" }
                    }
                ]
            }),
        ];

        let mut content = String::new();
        for l in lines {
            content.push_str(&l.to_string());
            content.push('\n');
        }
        fs::write(&transcript_file, content).await?;

        let importer = AntigravityImporter::with_dir(dir.path());
        let store = InMemoryMemoryStore::new();

        let imported = importer.import_all(&store).await?;
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].path, "antigravity://sessions/sess_test_123");
        assert!(imported[0].content.contains("Hello Antigravity!"));
        assert!(!imported[0].content.contains("\x1B[31m")); // ANSI stripped!
        assert!(imported[0].content.contains("view_file(Read config)"));

        Ok(())
    }
}
