//! Subprocess-based execution engine for plugins.

use crate::error::{GraphError, Result};
use crate::plugin::types::{FileToParse, PluginConfig, PluginEngine, PluginRequest, PluginResponse};
use crate::types::{Language, Symbol};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::instrument;

/// Subprocess-based execution engine for plugins.
#[derive(Default)]
pub struct ProcessEngine {
    monitor: Option<Arc<crate::plugin::health::PluginHealthMonitor>>,
}

impl ProcessEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn record_success(&self, name: &str) {
        if let Some(monitor) = &self.monitor {
            monitor.record_success(name);
        }
    }

    fn record_failure(&self, name: &str, error: String) {
        if let Some(monitor) = &self.monitor {
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
                .map_err(|e| {
                    GraphError::Parser(format!("failed to read plugin stdout: {}", e))
                })?;

            let mut stderr = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .await
                .ok();

            let status = child.wait().await.map_err(|e| {
                GraphError::Parser(format!("plugin process failed to exit: {}", e))
            })?;

            if !status.success() {
                let err = format!(
                    "plugin '{}' exited with status {}: {}",
                    command, status, stderr
                );
                engine.record_failure(&plugin_name, err.clone());
                return Err(GraphError::Parser(err));
            }

            let response: PluginResponse = serde_json::from_slice(&stdout).map_err(|e| {
                GraphError::Parser(format!(
                    "failed to parse plugin response: {}\nOutput was: {}",
                    e,
                    String::from_utf8_lossy(&stdout)
                ))
            })?;

            let results = response.results;
            engine.record_success(&plugin_name);

            // Convert Node to Symbol
            let symbols = results.into_iter().map(|n| {
                Symbol {
                    id: None,
                    stable_id: Some(n.id),
                    name: n.name,
                    kind: n.kind,
                    lang: lang.clone(),
                    file_path: n.file_path,
                    start_line: n.position.start_line,
                    end_line: n.position.end_line,
                    start_col: n.position.start_col,
                    end_col: n.position.end_col,
                    signature: n.signature,
                    parent: n.parent_id,
                    complexity: n.modifiers.get("complexity").and_then(|v| v.as_f64()).map(|f| f as f32),
                }
            }).collect();

            Ok(symbols)
        })
    }

    fn set_monitor(&self, _monitor: Arc<crate::plugin::health::PluginHealthMonitor>) {
    }
}

impl ProcessEngine {
    fn clone_shim(&self) -> Self {
        Self {
            monitor: self.monitor.clone(),
        }
    }
}
