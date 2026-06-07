//! Workspace operations and utilities
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::store::{FileMemoryStore, MemoryStore};
use crate::settings::XavierSettings;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;

#[derive(Debug)]
pub struct FileMigrationResult {
    pub migrated: bool,
    pub detail: String,
}

pub fn resolve_file_store_path(workspace_root: &Path) -> PathBuf {
    let settings = XavierSettings::current();
    let path = PathBuf::from(&settings.memory.file_path);
    if path.is_absolute() {
        path
    } else {
        workspace_root.join(&settings.memory.file_path)
    }
}

pub fn durable_migration_marker_path(file_store_path: &Path) -> PathBuf {
    file_store_path.with_extension("durable.migrated.json")
}

pub async fn migrate_file_store_if_needed(
    workspace_id: &str,
    file_store_path: &Path,
    marker_path: &Path,
    target_store: Arc<dyn MemoryStore>,
) -> Result<FileMigrationResult> {
    let legacy_marker_path = file_store_path.with_extension("surreal.migrated.json");
    let active_marker_path = if fs::try_exists(marker_path).await.unwrap_or(false) {
        marker_path.to_path_buf()
    } else if fs::try_exists(&legacy_marker_path).await.unwrap_or(false) {
        legacy_marker_path
    } else {
        marker_path.to_path_buf()
    };

    if fs::try_exists(&active_marker_path).await.unwrap_or(false) {
        let detail = match fs::read_to_string(&active_marker_path).await {
            Ok(detail) => detail,
            Err(_) => "legacy durable-store migration already recorded".to_string(),
        };
        return Ok(FileMigrationResult {
            migrated: detail.contains("\"migrated\":true"),
            detail,
        });
    }

    if !fs::try_exists(file_store_path).await.unwrap_or(false) {
        return Ok(FileMigrationResult {
            migrated: false,
            detail: "no legacy file store found".to_string(),
        });
    }

    let legacy_store = FileMemoryStore::new(file_store_path).await?;
    let legacy_state = legacy_store.load_workspace_state(workspace_id).await?;
    let target_state = target_store.load_workspace_state(workspace_id).await?;

    let should_import = target_state.memories.is_empty()
        && target_state.beliefs.is_empty()
        && target_state.session_tokens.is_empty()
        && target_state.checkpoints.is_empty()
        && (!legacy_state.memories.is_empty()
            || !legacy_state.beliefs.is_empty()
            || !legacy_state.session_tokens.is_empty()
            || !legacy_state.checkpoints.is_empty());

    if should_import {
        for record in legacy_state.memories.clone() {
            target_store.put(record).await?;
        }
        target_store
            .save_beliefs(workspace_id, legacy_state.beliefs.clone())
            .await?;
        for token in legacy_state.session_tokens.clone() {
            target_store.save_session_token(workspace_id, token).await?;
        }
        for checkpoint in legacy_state.checkpoints.clone() {
            target_store
                .save_checkpoint(workspace_id, checkpoint)
                .await?;
        }
    }

    let detail = serde_json::json!({
        "migrated": should_import,
        "source": file_store_path.display().to_string(),
        "legacy_memories": legacy_state.memories.len(),
        "legacy_beliefs": legacy_state.beliefs.len(),
        "legacy_session_tokens": legacy_state.session_tokens.len(),
        "legacy_checkpoints": legacy_state.checkpoints.len(),
        "reason": if should_import { format!("imported legacy file store into {}", target_store.backend().as_str()) } else { "skipped legacy import because target store already contained data or file was empty".to_string() }
    }).to_string();
    fs::write(marker_path, &detail).await?;

    Ok(FileMigrationResult {
        migrated: should_import,
        detail,
    })
}
