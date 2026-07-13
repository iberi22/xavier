//! Subprocess plugin engine.
//!
//! Runs a plugin as an isolated OS process, sending a [`PluginRequest`] as JSON
//! over stdin and reading a [`PluginResponse`] from stdout. Enforces a timeout
//! (default 30s per the feature spec) and kills the child on drop so a wedged
//! plugin can never leak past its parse call.

use crate::error::{GraphError, Result};
use crate::plugin::types::{
    FileToParse, PluginConfig, PluginEngine, PluginHealth, PluginRequest, PluginResponse,
};
use crate::types::{Language, Symbol};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

/// Default per-call timeout. The legacy `PluginHost` used 5s; the plugin-system
/// feature spec mandates 30s to accommodate heavier community parsers.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Spawns plugin processes and speaks the stdin/stdout JSON protocol to them.
pub struct ProcessEngine {
    timeout: Duration,
    /// Aggregate health counters per plugin command (best-effort diagnostics).
    health: Arc<Mutex<HashMap<String, PluginHealth>>>,
}

impl Default for ProcessEngine {
    fn default() -> Self {
        Self::with_timeout(DEFAULT_TIMEOUT)
    }
}

impl ProcessEngine {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            health: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Snapshot of recorded health for a plugin command.
    pub fn health_for(&self, command: &str) -> PluginHealth {
        self.health
            .lock()
            .get(command)
            .cloned()
            .unwrap_or_default()
    }

    fn record_success(&self, command: &str) {
        self.health
            .lock()
            .entry(command.to_string())
            .or_default()
            .record_success();
    }

    fn record_failure(&self, command: &str, error: impl Into<String>) {
        self.health
            .lock()
            .entry(command.to_string())
            .or_default()
            .record_failure(error);
    }

    /// Internal entry point shared by the trait impl and tests.
    async fn parse_inner(
        &self,
        config: &PluginConfig,
        lang: Language,
        files: Vec<FileToParse>,
    ) -> Result<Vec<Symbol>> {
        let request = PluginRequest { language: lang, files };
        let input_json = serde_json::to_string(&request)
            .map_err(|e| GraphError::Parser(e.to_string()))?;

        let mut child = tokio::process::Command::new(&config.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(GraphError::Io)?;

        // Feed stdin and close it so the plugin sees EOF.
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| GraphError::Parser("plugin stdin unavailable".to_string()))?;
            tokio::io::AsyncWriteExt::write_all(&mut stdin, input_json.as_bytes())
                .await
                .map_err(GraphError::Io)?;
            // stdin dropped here → child receives EOF.
        }

        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                self.record_failure(&config.command, e.to_string());
                return Err(GraphError::Io(e));
            }
            Err(_) => {
                // `wait_with_output` consumes the child; rely on `kill_on_drop`
                // to terminate it once the join handle is released. The timeout
                // is the contract the plugin agreed to.
                let msg = format!("plugin '{}' timed out after {:?}", config.command, self.timeout);
                self.record_failure(&config.command, &msg);
                return Err(GraphError::Parser(msg));
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = format!("plugin '{}' exited {}: {}", config.command, output.status, stderr);
            self.record_failure(&config.command, &msg);
            return Err(GraphError::Parser(msg));
        }

        let response: PluginResponse = serde_json::from_slice(&output.stdout)
            .map_err(|e| {
                let msg = format!("plugin '{}' returned invalid JSON: {}", config.command, e);
                self.record_failure(&config.command, &msg);
                GraphError::Parser(msg)
            })?;

        let result = response.into_result();
        match &result {
            Ok(symbols) => {
                debug!(
                    command = %config.command,
                    count = symbols.len(),
                    "plugin parse succeeded"
                );
                self.record_success(&config.command);
            }
            Err(e) => self.record_failure(&config.command, e.to_string()),
        }
        result
    }
}

impl PluginEngine for ProcessEngine {
    fn parse(
        &self,
        config: &PluginConfig,
        lang: Language,
        files: Vec<FileToParse>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Symbol>>> + Send>> {
        // `to_owned` so the future is `'static` and `Send`.
        let config = config.clone();
        let this_timeout = self.timeout;
        // Reconstruct a lightweight self-reference: ProcessEngine state lives
        // behind Arc, so clone the Arc for the future.
        let health = self.health.clone();
        Box::pin(async move {
            let engine = ProcessEngine { timeout: this_timeout, health };
            engine.parse_inner(&config, lang, files).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_is_30_seconds_per_spec() {
        assert_eq!(ProcessEngine::default().timeout, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn missing_plugin_binary_is_an_error_not_a_panic() {
        let engine = ProcessEngine::default();
        let config = PluginConfig {
            command: "this-binary-does-not-exist-anywhere-12345".into(),
            version: "0.0.0".into(),
            languages: vec![Language::Python],
            capabilities: vec!["parse".into()],
        };
        let result = engine
            .parse_inner(&config, Language::Python, vec![])
            .await;
        // The contract: a bad/missing plugin must surface as an Err, never a
        // panic. We assert the error but not the exact failure_count, because
        // on Windows a missing command may resolve to a shell stub that exits
        // before our JSON validation runs (which still records a failure), or
        // in some environments succeed unexpectedly — the durable contract is
        // the Err return, not the diagnostic counter.
        assert!(
            result.is_err(),
            "missing plugin should error, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn trait_dispatch_runs_the_same_engine() {
        // Exercise the boxed-future trait path with a guaranteed-missing binary
        // so we cover the `PluginEngine` indirection without depending on an
        // external plugin process.
        use crate::plugin::types::PluginEngine as _;
        let engine = ProcessEngine::default();
        let config = PluginConfig {
            command: "this-binary-does-not-exist-anywhere-trait".into(),
            version: "0.0.0".into(),
            languages: vec![Language::Python],
            capabilities: vec!["parse".into()],
        };
        let result = engine
            .parse(&config, Language::Python, vec![])
            .await;
        assert!(result.is_err());
    }
}
