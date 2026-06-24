use anyhow::{Result, anyhow};
use crate::settings::XavierSettings;
use crate::settings::types::{PluginConfig, McpServerConfig};
use std::process::Command;
use std::collections::HashMap;

pub struct PluginManager;

impl PluginManager {
    pub async fn install(name: &str) -> Result<()> {
        match name {
            "codegraph" => {
                println!("Installing codegraph plugin...");
                let status = Command::new("cargo")
                    .args(["install", "codegraph"])
                    .status()?;

                if !status.success() {
                    return Err(anyhow!("Failed to install codegraph via cargo"));
                }

                Self::configure_codegraph_mcp().await?;
                println!("✅ codegraph installed and configured as MCP server");
                Ok(())
            }
            _ => Err(anyhow!("Plugin '{}' not supported for auto-install", name)),
        }
    }

    pub async fn remove(name: &str) -> Result<()> {
        let mut settings = XavierSettings::current();
        let mut found = false;

        settings.plugins.installed.retain(|p| {
            if p.name == name {
                found = true;
                false
            } else {
                true
            }
        });

        if name == "codegraph" {
            settings.plugins.mcp_servers.remove("codegraph");
        }

        if found {
            settings.save().await?;
            println!("✅ Plugin '{}' removed from configuration", name);
        }

        Ok(())
    }

    pub fn list() -> Result<Vec<String>> {
        let settings = XavierSettings::current();
        let mut list = Vec::new();

        for plugin in &settings.plugins.installed {
            list.push(format!("{} (v{})", plugin.name, plugin.version));
        }

        for mcp in settings.plugins.mcp_servers.keys() {
            if !list.iter().any(|s| s.contains(mcp)) {
                list.push(format!("{} (mcp)", mcp));
            }
        }

        Ok(list)
    }

    pub async fn health_check(name: &str) -> Result<bool> {
        let settings = XavierSettings::current();

        if name == "codegraph" {
            if let Some(config) = settings.plugins.mcp_servers.get("codegraph") {
                let status = Command::new(&config.command)
                    .arg("--version")
                    .status();
                return Ok(status.map(|s| s.success()).unwrap_or(false));
            }
        }

        if settings.plugins.installed.iter().any(|p| p.name == name) {
            return Ok(true);
        }

        Ok(false)
    }

    async fn configure_codegraph_mcp() -> Result<()> {
        let mut settings = XavierSettings::current();

        // Add to installed plugins
        if !settings.plugins.installed.iter().any(|p| p.name == "codegraph") {
            settings.plugins.installed.push(PluginConfig {
                name: "codegraph".to_string(),
                version: "0.1.0".to_string(),
                enabled: true,
            });
        }

        // Configure MCP server
        let mut env = HashMap::new();
        if let Ok(token) = std::env::var("XAVIER_TOKEN") {
            env.insert("XAVIER_TOKEN".to_string(), token);
        }

        settings.plugins.mcp_servers.insert("codegraph".to_string(), McpServerConfig {
            command: "codegraph".to_string(),
            args: vec!["mcp".to_string()],
            env,
            disabled: false,
            auto_approve: vec![],
        });

        settings.save().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_plugins() {
        let plugins = PluginManager::list().unwrap();
        assert!(!plugins.is_empty());
        assert_eq!(plugins[0], "codegraph (mcp)");
    }

    #[tokio::test]
    async fn test_health_check() {
        let healthy = PluginManager::health_check("codegraph").await.unwrap();
        assert!(healthy);
    }
}
