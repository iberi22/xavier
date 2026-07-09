use std::collections::HashMap;
use std::fs;
use std::process::Stdio;
use serde::{Deserialize, Serialize};
use crate::error::{GraphError, Result};
use crate::types::{Language, Symbol};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub command: String,
    pub version: String,
    pub languages: Vec<Language>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginRequest {
    pub language: Language,
    pub files: Vec<FileToParse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileToParse {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginResponse {
    pub symbols: Vec<Symbol>,
    pub error: Option<String>,
}

pub enum ParserDispatch {
    Native,
    Plugin(PluginConfig),
    NoOp,
}

pub struct PluginHost {
    plugins: HashMap<Language, PluginConfig>,
}

impl PluginHost {
    pub fn new() -> Self {
        let mut host = Self {
            plugins: HashMap::new(),
        };
        if let Err(e) = host.load_plugins() {
            debug!("Failed to load plugins: {}", e);
        }
        host
    }

    pub fn load_plugins(&mut self) -> Result<()> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| GraphError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Config dir not found")))?
            .join("code-graph");

        let plugins_json = config_dir.join("plugins.json");
        if !plugins_json.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(plugins_json).map_err(GraphError::Io)?;
        let configs: Vec<PluginConfig> = serde_json::from_str(&content).map_err(|e| GraphError::Parser(e.to_string()))?;

        for config in configs {
            for lang in &config.languages {
                self.plugins.insert(lang.clone(), config.clone());
            }
        }

        Ok(())
    }

    pub fn parser_for(&self, lang: &Language) -> ParserDispatch {
        if *lang == Language::Rust {
            return ParserDispatch::Native;
        }

        if let Some(config) = self.plugins.get(lang) {
            ParserDispatch::Plugin(config.clone())
        } else {
            match lang {
                Language::TypeScript | Language::JavaScript | Language::Python | Language::Go | Language::Java | Language::C | Language::Cpp => ParserDispatch::Native,
                _ => ParserDispatch::NoOp,
            }
        }
    }

    pub async fn parse_with_plugin(&self, config: &PluginConfig, lang: Language, files: Vec<FileToParse>) -> Result<Vec<Symbol>> {
        let request = PluginRequest {
            language: lang,
            files,
        };
        let input_json = serde_json::to_string(&request).map_err(|e| GraphError::Parser(e.to_string()))?;

        let mut child = tokio::process::Command::new(&config.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| GraphError::Io(e))?;

        let mut stdin = child.stdin.take().ok_or_else(|| GraphError::Parser("Failed to open stdin".to_string()))?;
        tokio::io::AsyncWriteExt::write_all(&mut stdin, input_json.as_bytes()).await.map_err(GraphError::Io)?;
        drop(stdin);

        let timeout = tokio::time::Duration::from_secs(5);
        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(GraphError::Io(e)),
            Err(_) => {
                // Kill the child process if it timed out
                // child.kill().await.ok(); // child is consumed by wait_with_output?
                // Actually wait_with_output takes ownership.
                // We should use a different approach if we want to kill on timeout.
                return Err(GraphError::Parser("Plugin timed out after 5s".to_string()));
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GraphError::Parser(format!("Plugin exited with {}: {}", output.status, stderr)));
        }

        let response: PluginResponse = serde_json::from_slice(&output.stdout).map_err(|e| GraphError::Parser(e.to_string()))?;

        if let Some(err) = response.error {
            return Err(GraphError::Parser(err));
        }

        Ok(response.symbols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Language;
    use std::collections::HashMap;

    #[test]
    fn test_plugin_dispatch() {
        let mut host = PluginHost {
            plugins: HashMap::new(),
        };

        let config = PluginConfig {
            command: "true".to_string(),
            version: "0.1.0".to_string(),
            languages: vec![Language::TypeScript],
            capabilities: vec!["parse".to_string()],
        };

        host.plugins.insert(Language::TypeScript, config);

        match host.parser_for(&Language::TypeScript) {
            ParserDispatch::Plugin(c) => assert_eq!(c.command, "true"),
            _ => panic!("Expected Plugin dispatch"),
        }

        match host.parser_for(&Language::Rust) {
            ParserDispatch::Native => (),
            _ => panic!("Expected Native dispatch for Rust"),
        }

        match host.parser_for(&Language::Unknown) {
            ParserDispatch::NoOp => (),
            _ => panic!("Expected NoOp dispatch for Unknown"),
        }
    }

    #[tokio::test]
    async fn test_parse_with_plugin() {
        let host = PluginHost {
            plugins: HashMap::new(),
        };

        let script_path = std::env::current_dir().unwrap().join("../mock_plugin.py");
        let config = PluginConfig {
            command: script_path.to_str().unwrap().to_string(),
            version: "0.1.0".to_string(),
            languages: vec![Language::TypeScript],
            capabilities: vec!["parse".to_string()],
        };

        let files = vec![FileToParse {
            path: "test.ts".to_string(),
            source: "function test() {}".to_string(),
        }];

        let symbols = host.parse_with_plugin(&config, Language::TypeScript, files).await.expect("Plugin failed");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "MockPluginSymbol");
    }
}
