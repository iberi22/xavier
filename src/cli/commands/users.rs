//! CLI user account administration commands
//!
//! Handles the `xavier users` subcommand: listing local accounts, changing an
//! account role by email, and resetting an account password by email.
//!
//! These commands open `<estado>/.xavier/auth.db` directly — the same database
//! the HTTP server uses (`XAVIER_STATE_DIR`, else `HOME`, else `.`) — so they
//! work without the server running. They are meant for the node operator.

use crate::cli::commands::enums::UsersCommand;
use anyhow::{Context, Result};
use std::path::PathBuf;
use xavier::auth2::db::{AuthDb, UserSummary};

/// Resolve the auth database path exactly like the HTTP server does
/// (`src/cli/server.rs`): `XAVIER_STATE_DIR`, else `HOME`/`USERPROFILE`,
/// else the current directory, plus `.xavier/auth.db`.
fn auth_db_path() -> PathBuf {
    let state_dir_str = std::env::var("XAVIER_STATE_DIR")
        .or_else(|_| std::env::var("HOME"))
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(&state_dir_str).join(".xavier").join("auth.db")
}

fn open_auth_db() -> Result<AuthDb> {
    let path = auth_db_path();
    AuthDb::new(&path)
        .with_context(|| format!("no se pudo abrir la base de cuentas {}", path.display()))
}

/// Dispatch a [`UsersCommand`] to the appropriate handler.
pub async fn handle_users_command(cmd: UsersCommand) -> Result<()> {
    match cmd {
        UsersCommand::List { json } => list_users(json).await,
        UsersCommand::SetRole { email, role, json } => set_role(&email, &role, json).await,
        UsersCommand::ResetPassword { email, json } => reset_password(&email, json).await,
    }
}

async fn list_users(json: bool) -> Result<()> {
    let db = open_auth_db()?;
    let users: Vec<UserSummary> = db.list_user_summaries()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&users)?);
        return Ok(());
    }
    if users.is_empty() {
        println!("No hay cuentas registradas.");
        return Ok(());
    }
    println!("{:<38} {:<40} {:<10}", "ID", "EMAIL", "ROL");
    for u in &users {
        println!("{:<38} {:<40} {:<10}", u.id, u.email, u.role);
    }
    println!("\n{} cuenta(s).", users.len());
    Ok(())
}

async fn set_role(email: &str, role: &str, json: bool) -> Result<()> {
    let db = open_auth_db()?;
    let updated = db.set_user_role(email, role)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&updated)?);
        return Ok(());
    }
    println!(
        "Rol actualizado: {} ahora es '{}'.",
        updated.email, updated.role
    );
    Ok(())
}

async fn reset_password(email: &str, json: bool) -> Result<()> {
    let db = open_auth_db()?;
    // La contrasena en claro existe solo en esta variable: se imprime UNA vez
    // y jamas se registra en logs.
    let password = db.reset_user_password(email)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "email": email.trim().to_ascii_lowercase(),
                "password": password,
            }))?
        );
    } else {
        println!("Nueva contrasena para {}:", email.trim().to_ascii_lowercase());
        println!("{}", password);
    }
    eprintln!(
        "AVISO: esta contrasena no se vuelve a mostrar. Entregala a su dueno y pierde este texto."
    );
    Ok(())
}
