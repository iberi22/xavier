//! Workspace registry for project discovery
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::config::WorkspaceConfig;
use super::state::WorkspaceState;
use super::templates::seed_workspace;
use crate::agents::RuntimeConfig;
use crate::settings::XavierSettings;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceDbKind {
    Personal,
    Family,
    Org,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDb {
    pub db_id: String,
    pub db_path: String,
    pub display_name: String,
    pub kind: WorkspaceDbKind,
}

#[derive(Clone)]
pub struct WorkspaceContext {
    pub workspace_id: String,
    pub workspace: Arc<WorkspaceState>,
}

#[derive(Clone, Default)]
pub struct WorkspaceRegistry {
    pub(super) workspaces: Arc<RwLock<HashMap<String, Arc<WorkspaceState>>>>,
    pub(super) token_map: Arc<RwLock<HashMap<String, String>>>,
}

impl WorkspaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, workspace: WorkspaceState) -> Result<()> {
        let workspace = Arc::new(workspace);
        let workspace_id = workspace.config().id.clone();
        let token = workspace.config().token.clone();
        self.token_map
            .write()
            .await
            .insert(token, workspace_id.clone());
        self.workspaces
            .write()
            .await
            .insert(workspace_id, workspace);
        Ok(())
    }

    pub async fn authenticate(&self, token: &str) -> Option<WorkspaceContext> {
        if let Some(workspace_id) = self.token_map.read().await.get(token).cloned() {
            if let Some(workspace) = self.workspaces.read().await.get(&workspace_id).cloned() {
                return Some(WorkspaceContext {
                    workspace_id,
                    workspace,
                });
            }
        }
        let workspaces = self.workspaces.read().await;
        for (id, workspace) in workspaces.iter() {
            if workspace.is_session_token_valid(token).await {
                return Some(WorkspaceContext {
                    workspace_id: id.clone(),
                    workspace: workspace.clone(),
                });
            }
        }
        None
    }

    pub async fn default_context(&self) -> Option<WorkspaceContext> {
        let settings = XavierSettings::current();
        let preferred_id = settings.workspace.default_workspace_id.clone();
        let workspaces = self.workspaces.read().await;
        if let Some(workspace) = workspaces.get(&preferred_id).cloned() {
            return Some(WorkspaceContext {
                workspace_id: preferred_id,
                workspace,
            });
        }
        workspaces
            .iter()
            .next()
            .map(|(id, workspace)| WorkspaceContext {
                workspace_id: id.clone(),
                workspace: workspace.clone(),
            })
    }

    pub fn default_context_sync(&self) -> Option<WorkspaceContext> {
        let settings = XavierSettings::current();
        let preferred_id = settings.workspace.default_workspace_id.clone();
        let workspaces = self.workspaces.blocking_read();
        if let Some(workspace) = workspaces.get(&preferred_id).cloned() {
            return Some(WorkspaceContext {
                workspace_id: preferred_id,
                workspace,
            });
        }
        workspaces
            .iter()
            .next()
            .map(|(id, workspace)| WorkspaceContext {
                workspace_id: id.clone(),
                workspace: workspace.clone(),
            })
    }

    pub async fn default_from_env(runtime_config: RuntimeConfig) -> Result<Self> {
        let registry = Self::new();
        let config = WorkspaceConfig::from_env();
        let settings = XavierSettings::current();
        let workspace_root = PathBuf::from(&settings.memory.workspace_dir).join(&config.id);
        let workspace = WorkspaceState::new(config, runtime_config, workspace_root).await?;
        seed_workspace(&workspace).await?;
        registry.insert(workspace).await?;
        Ok(registry)
    }
}
