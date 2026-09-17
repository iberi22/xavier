//! Google Antigravity Session & Transcript Importer
//!
//! Scans ~/.gemini/antigravity/brain/*/logs/transcript.jsonl
//! and imports parsed, sanitized agent interaction turns into Xavier MemoryStore under antigravity:// paths.

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::memory::sanitizer::{RawTurn, SanitizeConfig, Sanitizer};
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

/// Importer for Google Antigravity transcripts and session logs.
pub struct AntigravityImporter {
    brains_dir: PathBuf,
    embedder: Option<Arc<dyn Embedder>>,
    sanitizer: Sanitizer,
}

impl Default for AntigravityImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl AntigravityImporter {
    /// Create a new importer with default brain directory (~/.gemini/antigravity/brain or ANTIGRAVITY_BRAIN_DIR).
    pub fn new() -> Self {
        let brains_dir = Self::resolve_brains_dir();
        Self {
            brains_dir,
            embedder: None,
            sanitizer: Sanitizer::new(SanitizeConfig::default()),
        }
    }

    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            brains_dir: path.as_ref().to_path_buf(),
            embedder: None,
            sanitizer: Sanitizer::new(SanitizeConfig::default()),
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    fn resolve_brains_dir() -> PathBuf {
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

    /// Scan  recursively for  files.
    pub async fn scan_transcripts(&self) -> Result<Vec<PathBuf>> {
        let mut transcripts = Vec::new();
        if !fs::try_exists(&self.brains_dir).await.unwrap_or(false) {
            debug!(
                "Antigravity brains dir {:?} does not exist",
                self.brains_dir
            );
            return Ok(transcripts);
        }

        let mut read_dir = fs::read_dir(&self.brains_dir).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let brain_path = entry.path();
            if brain_path.is_dir() {
                // Pattern 1: brain/<id>/.system_generated/logs/transcript.jsonl
                let nested_transcript = brain_path
                    .join(".system_generated")
                    .join("logs")
                    .join("transcript.jsonl");
                if fs::try_exists(&nested_transcript).await.unwrap_or(false) {
                    transcripts.push(nested_transcript);
                    continue;
                }

                // Pattern 2: brain/<id>/logs/transcript.jsonl
                let direct_logs = brain_path.join("logs").join("transcript.jsonl");
                if fs::try_exists(&direct_logs).await.unwrap_or(false) {
                    transcripts.push(direct_logs);
                    continue;
                }

                // Pattern 3: brain/<id>/transcript.jsonl
                let direct_file = brain_path.join("transcript.jsonl");
                if fs::try_exists(&direct_file).await.unwrap_or(false) {
                    transcripts.push(direct_file);
                }
            }
        }

        info!(
            "🔍 Discovered {} Antigravity transcript files",
            transcripts.len()
        );
        Ok(transcripts)
    }

    /// Parse a single JSON line into a RawTurn, discarding pure heartbeats or noise.
    pub fn parse_line(session_id: &str, turn_idx: usize, val: &Value) -> Option<RawTurn> {
        // Discard explicit heartbeat events
        if val.get("type").and_then(|t| t.as_str()) == Some("heartbeat") {
            return None;
        }

        let role = val
            .get("role")
            .or_else(|| val.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("user");
        let normalized_role = match role {
            "human" | "USER_INPUT" => "user",
            "ai" | "model" | "PLANNER_RESPONSE" => "assistant",
            "tool" | "TOOL_RESPONSE" => "tool",
            other => other,
        };

        // Extract text content
        let mut content = String::new();
        if let Some(c) = val.get("content").and_then(|c| c.as_str()) {
            content.push_str(c);
        } else if let Some(t) = val.get("text").and_then(|t| t.as_str()) {
            content.push_str(t);
        } else if let Some(m) = val.get("message").and_then(|m| m.as_str()) {
            content.push_str(m);
        } else if let Some(parts) = val.get("parts").and_then(|p| p.as_array()) {
            for part in parts {
                if let Some(txt) = part.get("text").and_then(|t| t.as_str()) {
                    content.push_str(txt);
                    content.push('\n');
                }
            }
        }

        if content.trim().is_empty() {
            return None;
        }

        let model = val
            .get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string());
        let project = val
            .get("cwd")
            .or_else(|| val.get("project"))
            .and_then(|p| p.as_str())
            .unwrap_or(session_id);

        let mut file_paths = Vec::new();
        if let Some(files) = val
            .get("files")
            .or_else(|| val.get("attachments"))
            .and_then(|a| a.as_array())
        {
            for f in files {
                if let Some(path_str) = f.as_str() {
                    file_paths.push(path_str.to_string());
                }
            }
        }

        Some(RawTurn {
            session_id: session_id.to_string(),
            turn_index: turn_idx,
            role: normalized_role.to_string(),
            content,
            timestamp: None,
            model,
            file_paths,
            meta: serde_json::json!({
                "source_app": "antigravity",
                "project": project,
            }),
        })
    }

    /// Import all discovered transcripts into the given MemoryStore.
    pub async fn import_all(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        let transcripts = self.scan_transcripts().await?;
        let mut all_imported = Vec::new();

        for path in transcripts {
            let session_id = path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("default_session")
                .to_string();

            let content = match fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Could not read transcript {:?}: {}", path, e);
                    continue;
                }
            };

            let mut raw_turns = Vec::new();
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(turn) = Self::parse_line(&session_id, idx, &val) {
                        raw_turns.push(turn);
                    }
                }
            }

            let report = self.sanitizer.sanitize_turns(&mut raw_turns);
            debug!(
                "Antigravity session {} sanitized: {} kept, {} dropped",
                session_id, report.kept, report.dropped_boilerplate
            );

            let workspace_id = "agent:antigravity".to_string();
            for turn in raw_turns {
                let record_path =
                    format!("antigravity://sessions/{}/{}", session_id, turn.turn_index);
                let mut record = MemoryRecord {
                    workspace_id: workspace_id.clone(),
                    path: record_path.clone(),
                    content: turn.content.clone(),
                    metadata: serde_json::json!({
                        "source_app": "antigravity",
                        "agent_id": session_id,
                        "project": turn.meta.get("project").and_then(|p| p.as_str()).unwrap_or("unknown"),
                        "model": turn.model.unwrap_or_else(|| "antigravity-unknown".to_string()),
                        "session_id": session_id,
                        "turn_index": turn.turn_index,
                        "file_paths": turn.file_paths,
                        "importer_version": env!("CARGO_PKG_VERSION"),
                    }),
                    ..Default::default()
                };

                record.id = stable_key("memory", &[&workspace_id, &record_path]);

                if let Some(embedder) = &self.embedder {
                    if let Ok(emb) = embedder.encode(&record.content).await {
                        record.embedding = emb;
                    }
                }

                store.put(record.clone()).await.context("put record")?;
                all_imported.push(record);
            }
        }

        info!(
            "✅ Successfully imported {} Antigravity turns",
            all_imported.len()
        );
        Ok(all_imported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_heartbeat_dropped() {
        let val = serde_json::json!({
            "type": "heartbeat",
            "timestamp": 123456789
        });
        assert!(AntigravityImporter::parse_line("brain-1", 0, &val).is_none());
    }

    #[test]
    fn test_parse_valid_user_turn() {
        let val = serde_json::json!({
            "role": "user",
            "content": "Fix the bug in src/main.rs and verify tests",
            "cwd": "/home/user/project"
        });
        let turn = AntigravityImporter::parse_line("brain-1", 1, &val).unwrap();
        assert_eq!(turn.role, "user");
        assert!(turn.content.contains("Fix the bug"));
        assert_eq!(turn.session_id, "brain-1");
    }
}
