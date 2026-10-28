use crate::cli::onboarding::SystemScanner;
use crate::settings::types::{ProviderConfigV2, XavierSettings};
use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, MultiSelect, Select};
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use uuid::Uuid;

/// Unattended setup never scans the machine or downloads models.
pub async fn handle_setup_options(
    local: bool,
    model: Option<String>,
    yes: bool,
    json: bool,
) -> Result<()> {
    use std::io::IsTerminal;
    if yes || json || !std::io::stdin().is_terminal() {
        let model = model.filter(|v| !v.trim().is_empty());
        if !local || model.is_none() {
            eprintln!("Unattended setup requires --local --model ID; use xavier setup --local --model ID --yes");
            std::process::exit(2);
        }
        return crate::cli::commands::llm::setup_local(model.unwrap(), json).await;
    }
    handle_setup(local, model).await
}

/// Handle setup.
pub async fn handle_setup(local: bool, model: Option<String>) -> Result<()> {
    if local {
        return handle_local_setup(model).await;
    }

    println!("\n━━━━ System Scan ━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let scan = SystemScanner::scan().await;

    println!("🖥️ OS: {}", scan.os);
    if let Some(gpu) = scan.hardware.gpu_name {
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

    let cli_agents_status = scan
        .cli_agents
        .iter()
        .map(|a| format!("{} {}", a.name, if a.installed { "✓" } else { "✗" }))
        .collect::<Vec<_>>()
        .join(" | ");
    println!("🧰 CLI Agents: {}", cli_agents_status);

    let api_keys_status = scan
        .api_keys
        .iter()
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

    config.fallback_chain = selected_indices
        .iter()
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
        println!("✅ Generated headless auth token (value hidden).");
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

    save_provider_config(&config)?;

    println!("\n✅ Xavier setup complete! Configuration saved to ~/.xavier/provider-config.yaml");

    Ok(())
}

async fn handle_local_setup(model: Option<String>) -> Result<()> {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🦙 Xavier 100% Local Setup Wizard");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // 1. Detect Ollama
    let scan = SystemScanner::scan().await;
    if !scan.ollama.running {
        println!("❌ Ollama is NOT running.");
        println!("\nTo use Xavier in local mode, you need Ollama installed and running.");
        println!("Download it from: https://ollama.com");
        println!("After installing, run 'ollama serve' and try again.");
        std::process::exit(1);
    }
    println!("✅ Ollama is running.");

    // 2. Determine LLM model
    let chosen_llm = if let Some(m) = model.filter(|s| !s.trim().is_empty()) {
        Some(m.trim().to_string())
    } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        let input: String = Input::new()
            .with_prompt("Local LLM model to use (any model your Ollama serves)")
            .allow_empty(true)
            .interact_text()?;
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    } else {
        None
    };

    let target_embed = "embeddinggemma";
    let has_embed = scan.ollama.models.iter().any(|m| m.contains(target_embed));

    if let Some(ref target_llm) = chosen_llm {
        let has_llm = scan.ollama.models.iter().any(|m| m.contains(target_llm));
        if !has_llm {
            if Confirm::new()
                .with_prompt(format!("Model {} not found. Pull it now?", target_llm))
                .default(true)
                .interact()?
            {
                pull_model(target_llm)?;
            } else {
                println!("⚠️ Skipping model pull. Xavier might not work correctly.");
            }
        } else {
            println!("✅ Model {} found.", target_llm);
        }
    } else {
        println!("No LLM model selected; skipping LLM pull. Set XAVIER_LOCAL_LLM_MODEL later.");
    }

    if !has_embed {
        if Confirm::new()
            .with_prompt(format!("Embedder {} not found. Pull it now?", target_embed))
            .default(true)
            .interact()?
        {
            pull_model(target_embed)?;
        } else {
            println!("⚠️ Skipping embedder pull. Xavier might not work correctly.");
        }
    } else {
        println!("✅ Embedder {} found.", target_embed);
    }

    // 3. Reachability test
    println!("\n🧪 Testing reachability...");
    match test_ollama_reachability(chosen_llm.as_deref(), target_embed).await {
        Ok(_) => println!("✅ Reachability test passed."),
        Err(e) => {
            println!("❌ Reachability test failed: {}", e);
            if !Confirm::new()
                .with_prompt("Continue anyway?")
                .default(false)
                .interact()?
            {
                return Ok(());
            }
        }
    }

    // 4. Write config
    println!("\n📝 Writing configurations...");

    // Update xavier.config.json
    let mut settings = XavierSettings::load()?.unwrap_or_default();
    settings.models.provider = "local".to_string();
    if let Some(ref target_llm) = chosen_llm {
        settings.models.local_llm_model = target_llm.to_string();
    }
    settings.models.local_llm_url = "http://localhost:11434/v1".to_string();
    settings.models.embedding_model = target_embed.to_string();
    settings.models.embedding_url = "http://localhost:11434/api/embeddings".to_string();
    settings.workspace.embedding_provider_mode = "local".to_string();

    settings.save().await?;
    println!("✅ config/xavier.config.json updated.");

    // Update .env
    write_env_local(chosen_llm.as_deref())?;
    println!("✅ .env updated with local-first section.");

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Xavier 100% local ready.");
    println!(
        "LLM: {} | Embeddings: {}",
        chosen_llm.as_deref().unwrap_or("none"),
        target_embed
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    Ok(())
}

fn pull_model(model: &str) -> Result<()> {
    println!("📥 Pulling {}... (this may take a few minutes)", model);
    let mut child = Command::new("ollama")
        .arg("pull")
        .arg(model)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to execute ollama pull")?;

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("ollama pull failed with status {}", status);
    }
    Ok(())
}

async fn test_ollama_reachability(model: Option<&str>, embed: &str) -> Result<()> {
    let client = reqwest::Client::new();

    // Test Chat if model configured
    if let Some(target_llm) = model {
        let chat_resp = client
            .post("http://localhost:11434/api/chat")
            .json(&serde_json::json!({
                "model": target_llm,
                "messages": [{"role": "user", "content": "hi"}],
                "stream": false
            }))
            .send()
            .await?;

        if !chat_resp.status().is_success() {
            anyhow::bail!("Chat API returned {}", chat_resp.status());
        }
    }

    // Test Embeddings
    let embed_resp = client
        .post("http://localhost:11434/api/embeddings")
        .json(&serde_json::json!({
            "model": embed,
            "prompt": "hello world"
        }))
        .send()
        .await?;

    if !embed_resp.status().is_success() {
        anyhow::bail!("Embeddings API returned {}", embed_resp.status());
    }

    Ok(())
}

fn write_env_local(llm_model: Option<&str>) -> Result<()> {
    let env_path = std::env::current_dir()?.join(".env");
    let mut content = if env_path.exists() {
        fs::read_to_string(&env_path)?
    } else {
        String::new()
    };

    let mut local_section = "\n# --- LOCAL-FIRST SETUP ---
XAVIER_MODEL_PROVIDER=local
XAVIER_LOCAL_LLM_URL=http://localhost:11434/v1
"
    .to_string();

    if let Some(m) = llm_model {
        local_section.push_str(&format!("XAVIER_LOCAL_LLM_MODEL={m}\n"));
    }

    local_section.push_str(
        "XAVIER_EMBEDDING_URL=http://localhost:11434/api/embeddings
XAVIER_EMBEDDING_MODEL=embeddinggemma
XAVIER_EMBEDDING_PROVIDER_MODE=local
",
    );

    if !content.contains("LOCAL-FIRST SETUP") {
        content.push_str(&local_section);
        fs::write(env_path, content)?;
    }

    Ok(())
}

fn save_provider_config(config: &ProviderConfigV2) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_write_env_local_logic() {
        let dir = tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        write_env_local(Some("test-model")).unwrap();

        let content = fs::read_to_string(dir.path().join(".env")).unwrap();
        assert!(content.contains("LOCAL-FIRST SETUP"));
        assert!(content.contains("XAVIER_MODEL_PROVIDER=local"));
        assert!(content.contains("XAVIER_LOCAL_LLM_MODEL=test-model"));
        assert!(content.contains("XAVIER_EMBEDDING_MODEL=embeddinggemma"));

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[tokio::test]
    async fn test_settings_update_logic() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("xavier.config.json");
        let state_path = dir.path().join("xavier.runtime.json");
        std::env::set_var("XAVIER_CONFIG_PATH", config_path.to_str().unwrap());
        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", state_path.to_str().unwrap());

        let mut settings = XavierSettings::default();
        settings.models.provider = "local".to_string();
        settings.models.local_llm_model = "test-model".to_string();
        settings.save().await.unwrap();

        // #2801: the write lands in the runtime state file; the config file
        // (read-only defaults) is never created or rewritten.
        let saved_content = fs::read_to_string(&state_path).unwrap();
        assert!(saved_content.contains("\"provider\": \"local\""));
        assert!(saved_content.contains("\"local_llm_model\": \"test-model\""));
        assert!(
            !config_path.exists(),
            "save() must not create the versioned config file"
        );

        std::env::remove_var("XAVIER_CONFIG_PATH");
        std::env::remove_var("XAVIER_RUNTIME_STATE_PATH");
    }
}
