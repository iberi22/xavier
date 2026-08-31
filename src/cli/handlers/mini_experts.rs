//! CLI handler for mini-expert commands.

use crate::cli::commands::enums::MiniExpertCommand;
use anyhow::Result;
use xavier::agents::mini_experts::{MiniExpertEntry, MiniExpertRegistry};

/// Handles mini-expert CLI subcommands.
pub async fn handle_mini_expert_command(cmd: MiniExpertCommand) -> Result<()> {
    let registry = MiniExpertRegistry::load_data_path();
    match cmd {
        MiniExpertCommand::Add {
            name,
            segment,
            language,
            clearance,
            source_dataset,
            model_gguf_path,
            provider,
            endpoint,
        } => {
            let entry = MiniExpertEntry {
                name: name.clone(),
                segment,
                language,
                clearance,
                source_dataset,
                model_gguf_path,
                provider,
                endpoint,
                api_key: None,
            };
            registry.register(entry)?;
            println!("Successfully registered mini-expert '{}'.", name);
        }
        MiniExpertCommand::List => {
            let list = registry.list();
            if list.is_empty() {
                println!("No mini-experts registered.");
            } else {
                println!("Registered Mini-Experts ({}):", list.len());
                for expert in list {
                    println!(
                        " - Name: {}, Segment: {}, Lang: {}, Clearance: {}, GGUF: {}, Provider: {}, Endpoint: {}",
                        expert.name,
                        expert.segment,
                        expert.language,
                        expert.clearance,
                        expert.model_gguf_path,
                        expert.provider,
                        expert.endpoint
                    );
                }
            }
        }
        MiniExpertCommand::Serve { name, port } => {
            let target_name = name.unwrap_or_else(|| "default".to_string());
            println!(
                "Serving mini-expert '{}' on local Ollama endpoint http://localhost:{}...",
                target_name, port
            );
        }
    }
    Ok(())
}
