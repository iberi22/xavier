use anyhow::Result;
use xavier::plugin_manager::PluginManager;
use colored::Colorize;
use crate::cli::commands::enums::PluginCommand;

pub async fn handle_plugin_command(cmd: PluginCommand) -> Result<()> {
    match cmd {
        PluginCommand::Install { name } => {
            println!("Installing plugin: {}...", name.cyan());
            PluginManager::install(&name).await?;
            println!("✅ Plugin {} installed successfully", name.green());
        }
        PluginCommand::Remove { name } => {
            println!("Removing plugin: {}...", name.red());
            PluginManager::remove(&name).await?;
            println!("✅ Plugin {} removed successfully", name.green());
        }
        PluginCommand::List => {
            println!("Installed Plugins:");
            let plugins = PluginManager::list()?;
            if plugins.is_empty() {
                println!("  No plugins installed.");
            } else {
                for plugin in plugins {
                    println!("  - {}", plugin.cyan());
                }
            }
        }
        PluginCommand::Health { name } => {
            println!("Checking health for plugin: {}...", name.cyan());
            let healthy = PluginManager::health_check(&name).await?;
            if healthy {
                println!("✅ Plugin {} is {}", name.cyan(), "HEALTHY".green());
            } else {
                println!("❌ Plugin {} is {}", name.cyan(), "UNHEALTHY".red());
            }
        }
    }
    Ok(())
}
