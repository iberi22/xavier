use crate::cli::onboarding::SystemScanner;
use crate::settings::types::ProviderConfigV2;
use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Select, MultiSelect};
use std::fs;
use uuid::Uuid;

pub async fn handle_setup() -> Result<()> {
    println!("\n━━━━ System Scan ━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let scan = SystemScanner::scan().await;

    println!("🖥️ OS: {}", scan.os);
    if let Some(gpu) = scan.gpu {
        println!("🎮 GPU: {}", gpu);
    } else {
        println!("🎮 GPU: Not detected");
    }

    let ollama_status = if scan.ollama.running {
        format!("RUNNING ({})", scan.ollama.models.join(", "))
    } else {
        "NOT RUNNING".to_string()
    };
    println!("🦙 Ollama: {}", ollama_status);

    let cli_agents_status = scan.cli_agents.iter()
        .map(|a| format!("{} {}", a.name, if a.installed { "✓" } else { "✗" }))
        .collect::<Vec<_>>()
        .join(" | ");
    println!("🧰 CLI Agents: {}", cli_agents_status);

    let api_keys_status = scan.api_keys.iter()
        .map(|k| format!("{} {}", k.name, if k.detected { "✓" } else { "✗" }))
        .collect::<Vec<_>>()
        .join(" | ");
    println!("🔑 API Keys: {}", api_keys_status);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let mut config = ProviderConfigV2::default();

    // 1. Active Provider
    let providers = vec!["auto", "openai", "anthropic", "groq", "local"];
    let selection = Select::new()
        .with_prompt("Choose active provider")
        .items(&providers)
        .default(0)
        .interact()?;
    config.active_provider = providers[selection].to_string();

    // 2. Auto Strategy
    if config.active_provider == "auto" {
        let strategies = vec!["lowest_latency", "highest_quality", "lowest_cost"];
        let selection = Select::new()
            .with_prompt("Choose auto-switch strategy")
            .items(&strategies)
            .default(0)
            .interact()?;
        config.auto_strategy = strategies[selection].to_string();
    }

    // 3. Fallback Chain
    let available_fallbacks = vec!["claude", "openai", "groq", "local"];
    let defaults = vec![true, true, true, false];
    let selected_indices = MultiSelect::new()
        .with_prompt("Configure fallback chain (Space to select, Enter to confirm)")
        .items(&available_fallbacks)
        .defaults(&defaults)
        .interact()?;

    config.fallback_chain = selected_indices.iter()
        .map(|&i| available_fallbacks[i].to_string())
        .collect();

    // 4. Headless Mode
    if Confirm::new()
        .with_prompt("Enable headless mode?")
        .default(true)
        .interact()?
    {
        config.headless.enabled = true;
        config.headless.port = Input::new()
            .with_prompt("Headless port")
            .default(8007)
            .interact_text()?;

        config.headless.auth_token = Uuid::new_v4().to_string();
        println!("✅ Generated auth token: {}", config.headless.auth_token);
    }

    // 5. Notifications
    config.notifications.provider_limit_warning = Confirm::new()
        .with_prompt("Enable provider limit warnings?")
        .default(true)
        .interact()?;

    config.notifications.new_model_detected = Confirm::new()
        .with_prompt("Enable notifications for new models detected?")
        .default(true)
        .interact()?;

    config.notifications.better_provider_available = Confirm::new()
        .with_prompt("Enable suggestions when a better provider is available?")
        .default(true)
        .interact()?;

    save_config(&config)?;

    println!("\n✅ Xavier setup complete! Configuration saved to ~/.xavier/provider-config.yaml");

    Ok(())
}

fn save_config(config: &ProviderConfigV2) -> Result<()> {
    let home = dirs::home_dir().context("could not find home directory")?;
    let xavier_dir = home.join(".xavier");
    if !xavier_dir.exists() {
        fs::create_dir_all(&xavier_dir)?;
    }
    let config_path = xavier_dir.join("provider-config.yaml");
    let yaml = serde_yaml::to_string(config)?;
    fs::write(config_path, yaml)?;
    Ok(())
}
