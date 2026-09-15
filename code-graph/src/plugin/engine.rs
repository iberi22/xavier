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
            let request = PluginRequest {
                language: lang.clone(),
                files,
            };

            let input = serde_json::to_string(&request)
                .map_err(|e| GraphError::Parser(format!("failed to serialize request: {}", e)))?;

            // Bound every plugin invocation: a misbehaving sidecar binary
            // (stale version, waiting on a socket, saturated host) must never
            // hang indexing forever. On timeout the child is killed and the
            // caller falls back to the next parser in the chain (Native).
            // Override with XAVIER_PLUGIN_PARSE_TIMEOUT_SECS (0 = use default).
            let timeout_secs = std::env::var("XAVIER_PLUGIN_PARSE_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(30);

            let mut child = Command::new(&command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| {
                    GraphError::Parser(format!("failed to spawn plugin '{}': {}", command, e))
                })?;

            let communicate = async {
                let mut stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| GraphError::Parser("plugin stdin unavailable".to_string()))?;
                stdin.write_all(input.as_bytes()).await.map_err(|e| {
                    GraphError::Parser(format!("failed to write to plugin stdin: {}", e))
                })?;
                drop(stdin);

                let mut stdout = Vec::new();
                child
                    .stdout
                    .take()
                    .ok_or_else(|| GraphError::Parser("plugin stdout unavailable".to_string()))?
                    .read_to_end(&mut stdout)
                    .await
                    .map_err(|e| {
                        GraphError::Parser(format!("failed to read plugin stdout: {}", e))
                    })?;

                let mut stderr = String::new();
                if let Some(mut err_pipe) = child.stderr.take() {
                    let _ = err_pipe.read_to_string(&mut stderr).await;
                }

                let status = child.wait().await.map_err(|e| {
                    GraphError::Parser(format!("plugin process failed to exit: {}", e))
                })?;

                if !status.success() {
                    return Err(GraphError::Parser(format!(
                        "plugin '{}' exited with status {}: {}",
                        command, status, stderr
                    )));
                }

                let response: PluginResponse = serde_json::from_slice(&stdout).map_err(|e| {
                    GraphError::Parser(format!(
                        "failed to parse plugin response: {}\nOutput was: {}",
                        e,
                        String::from_utf8_lossy(&stdout)
                    ))
                })?;

                Ok::<Vec<Symbol>, GraphError>(response.symbols)
            };

            match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), communicate)
                .await
            {
                Ok(Ok(symbols)) => {
                    engine.record_success(&plugin_name);
                    Ok(symbols)
                }
                Ok(Err(e)) => {
                    engine.record_failure(&plugin_name, e.to_string());
                    Err(e)
                }
                Err(_) => {
                    // Timeout: `communicate` is dropped (pipes closed), then
                    // ensure the child cannot linger as an orphan.
                    let _ = child.kill().await;
                    let err = format!(
                        "plugin '{}' parse timed out after {}s",
                        command, timeout_secs
                    );
                    engine.record_failure(&plugin_name, err.clone());
                    Err(GraphError::Parser(err))
                }
            }
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
