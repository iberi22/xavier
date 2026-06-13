use crate::cli::commands::enums::{WalletCommandSub, CommonsCommand};
use crate::data_commons::wallet::{XavierWallet, WalletConfig};
use xavier::mesh::node::NodeIdentity;
use anyhow::{Result, anyhow};
use dialoguer::{Password, Input};
use std::path::PathBuf;

pub async fn handle_wallet_command(cmd: WalletCommandSub) -> Result<()> {
    let config = WalletConfig {
        data_dir: dirs::config_dir()
            .ok_or_else(|| anyhow!("Could not find config dir"))?
            .join("xavier/wallet"),
        ..Default::default()
    };

    match cmd {
        WalletCommandSub::Create => {
            println!("✨ Creando nueva wallet post-cuántica $XAV...");
            let password = Password::new()
                .with_prompt("Contraseña para cifrar la wallet")
                .with_confirmation("Confirma la contraseña", "Las contraseñas no coinciden")
                .interact()?;

            let (wallet, phrase) = XavierWallet::create(config, &password)?;

            println!("\n✅ Wallet creada con éxito!");
            println!("📍 Dirección: {}", wallet.state.as_ref().unwrap().address.0);
            println!("\n⚠️  FRASE DE RECUPERACIÓN (Mnemonic):");
            println!("--------------------------------------------------");
            println!("{}", phrase);
            println!("--------------------------------------------------");
            println!("Guarda esta frase en un lugar seguro. Si la pierdes, no podrás recuperar tus fondos.");
        }
        WalletCommandSub::Status => {
            let password = Password::new()
                .with_prompt("Contraseña de la wallet")
                .interact()?;

            let wallet = XavierWallet::load(config, &password)?;
            let status = wallet.status();

            println!("\n📊 Estado de Wallet $XAV");
            println!("-------------------------");
            println!("Dirección:    {}", status.address.map(|a| a.0).unwrap_or_else(|| "N/A".to_string()));
            println!("Balance:      {} $XAV", status.balance);
            println!("Reputación:   {} (EigenTrust)", status.trust_score);
            println!("Contribución: {}", status.contribution_score);
            println!("Nodos vinculados: {}", status.node_count);
            println!("Hardware TPM: {}", if status.has_tpm { "✅ Sí" } else { "❌ No (Software)" });
        }
        WalletCommandSub::Import { seed } => {
            let password = Password::new()
                .with_prompt("Nueva contraseña para cifrar la wallet")
                .with_confirmation("Confirma la contraseña", "Las contraseñas no coinciden")
                .interact()?;

            let wallet = XavierWallet::from_seed(&seed, config, &password)?;
            wallet.save(&password)?;

            println!("\n✅ Wallet importada con éxito!");
            println!("📍 Dirección: {}", wallet.state.as_ref().unwrap().address.0);
        }
        WalletCommandSub::LinkNode => {
            let password = Password::new()
                .with_prompt("Contraseña de la wallet")
                .interact()?;

            let mut wallet = XavierWallet::load(config.clone(), &password)?;
            let identity = NodeIdentity::load_or_create()?;

            println!("🔗 Vinculando nodo {} a la wallet...", identity.node_id);

            let binding = wallet.register_node(identity.node_id.as_str())?;
            wallet.save(&password)?;

            println!("✅ Nodo vinculado con éxito el {}",
                chrono::DateTime::from_timestamp(binding.registered_at as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S")
            );
        }
    }
    Ok(())
}

pub async fn handle_commons_command(cmd: CommonsCommand) -> Result<()> {
    match cmd {
        CommonsCommand::Status => {
            println!("🌐 Xavier Data Commons — Estado de la Red");
            println!("Fase: 1 (Local Embeddings & Anonymous Export)");
            println!("Nodos activos: 1 (Tú)");
            println!("Contextos compartidos: 0");
        }
        _ => {
            println!("Feature en desarrollo para la Fase 2.");
        }
    }
    Ok(())
}
