use crate::cli::commands::enums::TelegramCommand;
use crate::secrets::telegram::TelegramBotTokenManager;
use anyhow::Result;
use dialoguer::{Input, Password};

pub async fn handle_telegram_command(cmd: TelegramCommand) -> Result<()> {
    let manager = TelegramBotTokenManager::new();
    match cmd {
        TelegramCommand::SetToken => {
            let token: String = Password::new()
                .with_prompt("Enter Telegram Bot Token")
                .interact()?;

            manager.store_token(&token)?;
            println!("✅ Telegram bot token stored and encrypted successfully.");
        }
        TelegramCommand::Status => {
            let token = manager.get_token()?;
            if let Some(_) = token {
                println!("✅ Telegram Bot Token: Configured (encrypted)");
            } else {
                println!("❌ Telegram Bot Token: Not configured");
            }

            let settings = crate::settings::XavierSettings::current();
            println!("Status: {}", if settings.telegram.enabled { "Enabled" } else { "Disabled" });
            println!("Mode: {}", settings.telegram.mode);
            if let Some(webhook) = &settings.telegram.webhook_url {
                println!("Webhook URL: {}", webhook);
            }
        }
    }
    Ok(())
}
