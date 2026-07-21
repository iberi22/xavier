//! Subprocess-based execution engine for plugins.

use crate::error::{GraphError, Result};
use crate::plugin::types::{
    FileToParse, PluginConfig, PluginEngine, PluginRequest, PluginResponse,
};
use crate::types::{Language, Symbol};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::instrument;

/// Subprocess-based execution engine for plugins.
pub struct ProcessEngine {
    monitor: parking_lot::RwLock<Option<Arc<crate::plugin::health::PluginHealthMonitor>>>,
}

impl Default for ProcessEngine {
    fn default() -> Self {
        Self {
            monitor: parking_lot::RwLock::new(None),
        }
    }
}

impl ProcessEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn record_success(&self, name: &str) {
        if let Some(monitor) = &*self.monitor.read() {
            monitor.record_success(name);
        }
    }

    fn record_failure(&self, name: &str, error: String) {
        if let Some(monitor) = &*self.monitor.read() {
            monitor.record_failure(name, error);
        }
    }
}

impl PluginEngine for ProcessEngine {
    #[instrument(skip(self, _config, files))]
    fn parse(
        &self,
        _config: &PluginConfig,
        lang: Language,
        files: Vec<FileToParse>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Symbol>>> + Send>> {
        let command = _config.command.clone();
        let plugin_name = _config.name.clone();
        let engine = self.clone_shim();

        Box::pin(async move {
            let res = async {
                let request = PluginRequest {
                    language: lang.clone(),
                    files,
                };

                let input = serde_json::to_string(&request)
                    .map_err(|e| GraphError::Parser(format!("failed to serialize request: {}", e)))?;

                let mut child = Command::new(&command)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| {
                        GraphError::Parser(format!("failed to spawn plugin '{}': {}", command, e))
                    })?;

                let mut stdin = child.stdin.take().unwrap();
                stdin.write_all(input.as_bytes()).await.map_err(|e| {
                    GraphError::Parser(format!("failed to write to plugin stdin: {}", e))
                })?;
                drop(stdin);

                let mut stdout = Vec::new();
                child
                    .stdout
                    .take()
                    .unwrap()
                    .read_to_end(&mut stdout)
                    .await
                    .map_err(|e| GraphError::Parser(format!("failed to read plugin stdout: {}", e)))?;

                let mut stderr = String::new();
                child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut stderr)
                    .await
                    .ok();

                let status = child
                    .wait()
                    .await
                    .map_err(|e| GraphError::Parser(format!("plugin process failed to exit: {}", e)))?;

                if !status.success() {
                    let err = format!(
                        "plugin '{}' exited with status {}: {}",
                        command, status, stderr
                    );
                    return Err(GraphError::Parser(err));
                }

                let response: PluginResponse = serde_json::from_slice(&stdout).map_err(|e| {
                    GraphError::Parser(format!(
                        "failed to parse plugin response: {}\nOutput was: {}",
                        e,
                        String::from_utf8_lossy(&stdout)
                    ))
                })?;

                Ok(response.symbols)
            }.await;

            match &res {
                Ok(_) => {
                    engine.record_success(&plugin_name);
                }
                Err(e) => {
                    engine.record_failure(&plugin_name, e.to_string());
                }
            }
            res
        })
    }

    fn set_monitor(&self, monitor: Arc<crate::plugin::health::PluginHealthMonitor>) {
        *self.monitor.write() = Some(monitor);
    }
}

impl ProcessEngine {
    fn clone_shim(&self) -> Self {
        Self {
            monitor: parking_lot::RwLock::new(self.monitor.read().clone()),
        }
    }
}
