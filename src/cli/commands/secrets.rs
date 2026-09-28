//! CLI commands for secret/ephemeral credential management (Clavis)
//!
//! Handles the `xavier secrets` subcommand for lending, listing, and
//! revoking ephemeral secret leases to/from agents.

use std::io::IsTerminal;
use std::path::Path;

use crate::cli::commands::enums::{SecretsCommand, VaultCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{resolve_base_url, xavier_token};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use xavier::secrets::import_env::{parse_env_reader, read_value_from_reader, SecretValue};
use xavier::secrets::vault::HardwareVault;

pub(crate) trait VaultOps {
    fn store_secret(&self, key: &str, value: &str) -> Result<()>;
    fn get_secret(&self, key: &str) -> Result<String>;
}

impl VaultOps for HardwareVault {
    fn store_secret(&self, key: &str, value: &str) -> Result<()> {
        HardwareVault::store_secret(self, key, value).map_err(|e| anyhow::anyhow!("{e}"))
    }
    fn get_secret(&self, key: &str) -> Result<String> {
        HardwareVault::get_secret(self, key).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

fn read_secret_value_from_stream<R: std::io::BufRead>(
    reader: &mut R,
    is_tty: bool,
) -> Result<SecretValue> {
    if is_tty {
        let password = dialoguer::Password::new()
            .with_prompt("Secret value")
            .interact()
            .context("failed to read secret from prompt")?;
        if password.is_empty() {
            anyhow::bail!("secret value cannot be empty");
        }
        Ok(SecretValue::new(password))
    } else {
        read_value_from_reader(reader).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

pub(crate) fn vault_set_with_reader<V: VaultOps, R: std::io::BufRead>(
    vault: &V,
    key: &str,
    reader: &mut R,
    is_tty: bool,
) -> Result<()> {
    let secret = read_secret_value_from_stream(reader, is_tty)?;
    vault.store_secret(key, secret.expose())?;
    println!("Secret '{}' stored in hardware vault.", key);
    Ok(())
}

pub(crate) fn format_vault_get_output(key: &str, backend: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let hash = hasher.finalize();
    let fp: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
    format!("{key}: backend={backend} fingerprint=sha256:{fp}")
}

pub(crate) fn vault_get_with_vault<V: VaultOps, W: std::io::Write>(
    vault: &V,
    key: &str,
    writer: &mut W,
) -> Result<()> {
    let value = vault.get_secret(key)?;
    let output = format_vault_get_output(key, "hardware-vault", &value);
    writeln!(writer, "{output}")?;
    Ok(())
}

/// Dispatch a [`VaultCommand`] to the appropriate handler.
pub async fn handle_vault_command(cmd: VaultCommand) -> Result<()> {
    let vault = HardwareVault::new("xavier");
    match cmd {
        VaultCommand::Set { key } => {
            let is_tty = std::io::stdin().is_terminal();
            let mut stdin = std::io::stdin().lock();
            vault_set_with_reader(&vault, &key, &mut stdin, is_tty)?;
        }
        VaultCommand::Get { key } => {
            let mut stdout = std::io::stdout();
            vault_get_with_vault(&vault, &key, &mut stdout)?;
        }
        VaultCommand::Delete { key } => {
            vault.delete_secret(&key)?;
            println!("Secret '{}' deleted from hardware vault.", key);
        }
    }
    Ok(())
}

pub(crate) fn put_secret_with_reader<V: VaultOps, R: std::io::BufRead>(
    vault: &V,
    name: &str,
    reader: &mut R,
    is_tty: bool,
) -> Result<()> {
    let secret = read_secret_value_from_stream(reader, is_tty)?;
    vault.store_secret(name, secret.expose())?;
    println!("Secret '{}' stored in vault.", name);
    Ok(())
}

async fn put_secret(name: &str) -> Result<()> {
    let vault = HardwareVault::new("xavier");
    let is_tty = std::io::stdin().is_terminal();
    let mut stdin = std::io::stdin().lock();
    put_secret_with_reader(&vault, name, &mut stdin, is_tty)
}

pub(crate) fn import_env_with_vault<V: VaultOps>(
    vault: &V,
    from: &Path,
    dry_run: bool,
    apply: bool,
) -> Result<()> {
    if dry_run == apply {
        anyhow::bail!("exactly one of --dry-run or --apply must be specified");
    }
    let file = std::fs::File::open(from)
        .with_context(|| format!("failed to open file '{}'", from.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let plan = parse_env_reader(&mut reader).map_err(|e| anyhow::anyhow!("{e}"))?;

    if dry_run {
        for entry in plan.entries() {
            println!("[dry-run] {} (line {})", entry.name(), entry.line());
        }
        println!(
            "Dry run complete: {} secret(s) validated, 0 stored.",
            plan.len()
        );
        return Ok(());
    }

    let mut failed = 0;
    for entry in plan.entries() {
        match vault.store_secret(entry.name(), entry.expose_value()) {
            Ok(()) => println!("[stored] {}", entry.name()),
            Err(_) => {
                eprintln!("[failed] {}", entry.name());
                failed += 1;
            }
        }
    }
    if failed > 0 {
        anyhow::bail!("{failed} secret(s) failed to store");
    }
    println!("Import complete: {} secret(s) stored.", plan.len());
    Ok(())
}

async fn import_env_cmd(from: &Path, dry_run: bool, apply: bool) -> Result<()> {
    let vault = HardwareVault::new("xavier");
    import_env_with_vault(&vault, from, dry_run, apply)
}

/// Dispatch a [`SecretsCommand`] to the appropriate handler.
pub async fn handle_secrets_command(cmd: SecretsCommand) -> Result<()> {
    match cmd {
        SecretsCommand::Put { name } => put_secret(&name).await,
        SecretsCommand::ImportEnv {
            from,
            dry_run,
            apply,
        } => import_env_cmd(&from, dry_run, apply).await,
        SecretsCommand::Lend {
            secret_name,
            agent,
            ttl,
        } => lend_secret(&secret_name, &agent, ttl).await,
        SecretsCommand::ListLeases => list_leases().await,
        SecretsCommand::Revoke { token } => revoke_lease(&token).await,
        SecretsCommand::Status { token } => check_lease_status(&token).await,
    }
}

async fn lend_secret(name: &str, agent: &str, ttl: u64) -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/lend", resolve_base_url());
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({
            "secret_name": name,
            "agent_id": agent,
            "ttl_seconds": ttl
        }))
        .send()
        .await?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await?;
        println!("Secret lent successfully!");
        println!("Lease Token: {}", body["token"]);
        println!("Expires: {}", body["expires_at"]);
    } else {
        println!("Failed to lend secret: {}", response.status());
    }
    Ok(())
}

async fn list_leases() -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/leases", resolve_base_url());
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .get(&url)
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let leases: Vec<serde_json::Value> = response.json().await?;
        println!(
            "{:<20} {:<20} {:<20} {:<10}",
            "Agent", "Secret", "Expires", "Status"
        );
        for lease in leases {
            println!(
                "{:<20} {:<20} {:<20} {:<10}",
                lease["agent_id"].as_str().unwrap_or("?"),
                lease["secret_name"].as_str().unwrap_or("?"),
                lease["expires_at"].as_str().unwrap_or("?"),
                if lease["revoked"].as_bool().unwrap_or(false) {
                    "Revoked"
                } else {
                    "Active"
                }
            );
        }
    } else {
        println!("Failed to list leases: {}", response.status());
    }
    Ok(())
}

async fn revoke_lease(token_str: &str) -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/revoke", resolve_base_url());
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({ "token": token_str }))
        .send()
        .await?;

    if response.status().is_success() {
        println!("Lease revoked successfully.");
    } else {
        println!("Failed to revoke lease: {}", response.status());
    }
    Ok(())
}

async fn check_lease_status(token_str: &str) -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/status/{}", resolve_base_url(), token_str);
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .get(&url)
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let status: serde_json::Value = response.json().await?;
        println!(
            "Lease Status: {}",
            if status["revoked"].as_bool().unwrap_or(false) {
                "Revoked"
            } else {
                "Active"
            }
        );
        println!("Agent: {}", status["agent_id"]);
        println!("Expires: {}", status["expires_at"]);
    } else {
        println!("Failed to get lease status: {}", response.status());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;

    #[derive(Default)]
    struct TestVault {
        secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
    }

    impl VaultOps for TestVault {
        fn store_secret(&self, key: &str, value: &str) -> Result<()> {
            self.secrets
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
        fn get_secret(&self, key: &str) -> Result<String> {
            self.secrets
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Secret not found: {key}"))
        }
    }

    #[test]
    fn test_put_reads_value_from_stdin() {
        let vault = TestVault::default();
        let key = "CANARY_PUT_KEY";
        let canary = "synthetic_canary_secret_value_111";

        let mut input = Cursor::new(format!("{canary}\n"));
        put_secret_with_reader(&vault, key, &mut input, false).expect("put secret");

        let stored = vault.get_secret(key).expect("retrieve stored secret");
        assert_eq!(stored, canary);
    }

    #[test]
    fn test_import_env_dry_run_writes_nothing() {
        let tmp = tempdir().expect("create tempdir");
        let vault = TestVault::default();

        let env_path = tmp.path().join(".env.canary");
        std::fs::write(
            &env_path,
            "CANARY_ALPHA=synthetic_alpha_val\nCANARY_BETA=synthetic_beta_val\n",
        )
        .expect("write canary .env");

        import_env_with_vault(&vault, &env_path, true, false).expect("dry run");

        assert!(vault.get_secret("CANARY_ALPHA").is_err());
        assert!(vault.get_secret("CANARY_BETA").is_err());
    }

    #[test]
    fn test_import_env_apply_stores_every_entry() {
        let tmp = tempdir().expect("create tempdir");
        let vault = TestVault::default();

        let env_path = tmp.path().join(".env.canary");
        std::fs::write(
            &env_path,
            "CANARY_ONE=synthetic_one_val\nCANARY_TWO=synthetic_two_val\n",
        )
        .expect("write canary .env");

        import_env_with_vault(&vault, &env_path, false, true).expect("apply");

        assert_eq!(
            vault.get_secret("CANARY_ONE").expect("get CANARY_ONE"),
            "synthetic_one_val"
        );
        assert_eq!(
            vault.get_secret("CANARY_TWO").expect("get CANARY_TWO"),
            "synthetic_two_val"
        );
    }

    #[test]
    fn test_vault_get_output_contains_no_value() {
        let vault = TestVault::default();
        let key = "CANARY_VAULT_GET_KEY";
        let canary = "synthetic_canary_get_secret_val_222";

        vault.store_secret(key, canary).expect("store secret");

        let mut out = Vec::new();
        vault_get_with_vault(&vault, key, &mut out).expect("vault get");
        let rendered = String::from_utf8(out).expect("valid utf-8");

        assert!(
            !rendered.contains(canary),
            "rendered output must NOT contain canary value; got: {rendered}"
        );
        assert!(
            rendered.contains("sha256:"),
            "rendered output must contain 'sha256:'; got: {rendered}"
        );
        assert!(
            rendered.contains(key),
            "rendered output must contain key; got: {rendered}"
        );
        assert!(
            rendered.contains("hardware-vault"),
            "rendered output must contain backend; got: {rendered}"
        );
    }

    #[test]
    fn test_vault_set_has_no_value_argument() {
        use clap::{CommandFactory, Parser};

        let app = crate::cli::state::Cli::command();

        let vault_cmd = app
            .get_subcommands()
            .find(|c| c.get_name() == "vault")
            .expect("vault subcommand exists");
        let set_cmd = vault_cmd
            .get_subcommands()
            .find(|c| c.get_name() == "set")
            .expect("vault set subcommand exists");

        let arg_names: Vec<&str> = set_cmd
            .get_arguments()
            .map(|a| a.get_id().as_str())
            .collect();
        assert!(
            !arg_names.contains(&"value"),
            "vault set must not have a 'value' argument; args: {arg_names:?}"
        );

        let positionals: Vec<&str> = set_cmd
            .get_positionals()
            .map(|a| a.get_id().as_str())
            .collect();
        assert_eq!(
            positionals,
            vec!["key"],
            "vault set positionals must only be ['key']"
        );

        let parse_err = crate::cli::state::Cli::try_parse_from([
            "xavier",
            "vault",
            "set",
            "mykey",
            "some_illegal_value",
        ]);
        assert!(
            parse_err.is_err(),
            "passing two positionals to 'vault set' must be rejected"
        );
    }
}
