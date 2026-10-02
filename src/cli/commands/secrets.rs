//! CLI commands for secret/ephemeral credential management (Clavis)
//!
//! Handles the `xavier secrets` subcommand for lending, listing, and
//! revoking ephemeral secret leases to/from agents.

use std::io::IsTerminal;
use std::path::Path;

use crate::cli::commands::enums::{KeysCommand, SecretsCommand, VaultCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{resolve_base_url, xavier_token};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use xavier::clavis;
use xavier::secrets::import_env::{parse_env_reader, read_value_from_reader, SecretValue};
use xavier::secrets::keygen::{self, KeyScope};
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

/// Vault service name used for both `xavier vault` and `xavier keys`.
const VAULT_SERVICE: &str = "xavier";

/// Resolve the TTL: explicit flag wins, otherwise the env-configured default.
fn resolve_ttl(ttl_secs: Option<u64>) -> Result<u64> {
    match ttl_secs {
        Some(0) | None => keygen::default_ttl_secs().map_err(|e| anyhow::anyhow!("{e}")),
        Some(n) => Ok(n),
    }
}

/// `xavier keys generate`: mint a key, store it in the vault and print only
/// the redacted line. The plaintext never reaches `writer`.
pub(crate) fn keys_generate_with_vault<V: keygen::KeyVault, W: std::io::Write>(
    vault: &V,
    name: &str,
    scope: KeyScope,
    ttl_secs: u64,
    writer: &mut W,
) -> Result<()> {
    let key = keygen::generate_key(vault, name, scope, ttl_secs, chrono::Utc::now())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    writeln!(writer, "{}", keygen::format_generate_output(&key))?;
    writeln!(
        writer,
        "The value was NOT printed. Retrieve it once with: xavier keys export --name {name}"
    )?;
    Ok(())
}

/// `xavier keys export`: the only path that writes a plaintext key. It fails
/// closed when stdout is not a TTY and no `--yes` was given, and it always
/// registers the value in the global log masker first.
pub(crate) fn keys_export_with_vault<V: keygen::KeyVault, W: std::io::Write>(
    vault: &V,
    name: &str,
    confirm: Option<bool>,
    is_tty: bool,
    writer: &mut W,
) -> Result<()> {
    let value = keygen::export_key(vault, name).map_err(|e| anyhow::anyhow!("{e}"))?;
    clavis::register_secret(&value);

    match confirm {
        Some(true) => {}
        Some(false) => anyhow::bail!("export cancelled; the key was not printed"),
        None => {
            if !is_tty {
                anyhow::bail!(
                    "refusing to print the key '{name}' to a non-interactive stdout; \
                     re-run with --yes if this is intended"
                );
            }
            let ok = dialoguer::Confirm::new()
                .with_prompt(format!(
                    "Print the plaintext of API key '{name}' to stdout?"
                ))
                .default(false)
                .interact()
                .context("failed to read confirmation")?;
            if !ok {
                anyhow::bail!("export cancelled; the key was not printed");
            }
        }
    }

    writeln!(writer, "{value}")?;
    Ok(())
}

/// `xavier keys list`: metadata only, never a value.
pub(crate) fn keys_list_with_vault<V: keygen::KeyVault, W: std::io::Write>(
    vault: &V,
    name: &str,
    writer: &mut W,
) -> Result<()> {
    let meta = keygen::load_metadata(vault, name).ok_or_else(|| {
        anyhow::anyhow!(
            "no API key named '{name}' in the vault (generate one with 'xavier keys generate')"
        )
    })?;
    let expiry = meta
        .expires_at
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| "never".to_string());
    writeln!(
        writer,
        "{}: env={} backend=hardware-vault fingerprint={} ttl_secs={} expires_at={} rotation={} value=<hidden>",
        meta.name,
        meta.scope,
        meta.fingerprint,
        meta.ttl_secs,
        expiry,
        meta.rotation_count,
    )?;
    Ok(())
}

/// `xavier keys revoke`: drop the key and its metadata from the vault.
pub(crate) fn keys_revoke_with_vault<V: keygen::KeyVault, W: std::io::Write>(
    vault: &V,
    name: &str,
    writer: &mut W,
) -> Result<()> {
    keygen::revoke_key(vault, name).map_err(|e| anyhow::anyhow!("{e}"))?;
    writeln!(writer, "API key '{name}' revoked from the hardware vault.")?;
    Ok(())
}

/// Dispatch a [`KeysCommand`] to the appropriate handler.
pub async fn handle_keys_command(cmd: KeysCommand) -> Result<()> {
    let vault = HardwareVault::new(VAULT_SERVICE);
    let mut stdout = std::io::stdout();
    match cmd {
        KeysCommand::Generate {
            name,
            ttl_secs,
            test,
        } => {
            let scope = if test { KeyScope::Test } else { KeyScope::Live };
            keys_generate_with_vault(&vault, &name, scope, resolve_ttl(ttl_secs)?, &mut stdout)
        }
        KeysCommand::Export { name, yes } => keys_export_with_vault(
            &vault,
            &name,
            if yes { Some(true) } else { None },
            std::io::stdout().is_terminal(),
            &mut stdout,
        ),
        KeysCommand::List { name } => keys_list_with_vault(&vault, &name, &mut stdout),
        KeysCommand::Revoke { name } => keys_revoke_with_vault(&vault, &name, &mut stdout),
    }
}

/// Dispatch a [`VaultCommand`] to the appropriate handler.
pub async fn handle_vault_command(cmd: VaultCommand) -> Result<()> {
    let vault = HardwareVault::new(VAULT_SERVICE);
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

/// Environment variable the child receives the secret in. The value itself
/// never reaches this process: the server injects it into the child's
/// environment and reports only the exit code, lease token and duration.
pub(crate) const DEFAULT_EXEC_ENV_VAR: &str = "XAVIER_SECRET";

/// Build the `POST /secrets/exec` request body.
///
/// `args` stays a JSON array and no field concatenates the command with its
/// arguments, so the request cannot be reinterpreted as a shell string.
pub(crate) fn exec_request_body(
    secret_name: &str,
    agent: &str,
    ttl: u64,
    env_var: &str,
    command: &str,
    args: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "secret_name": secret_name,
        "agent_id": agent,
        "ttl_seconds": ttl,
        "env_var": env_var,
        "command": command,
        "args": args,
    })
}

/// Run `command` on the server with a vault secret injected into its
/// environment. Only the exit code, the lease token and the duration are
/// printed: no value is ever received by this process.
async fn exec_with_secret(
    secret_name: &str,
    agent: &str,
    ttl: u64,
    env_var: &str,
    command: &str,
    args: &[String],
) -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/exec", resolve_base_url());
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&exec_request_body(
            secret_name,
            agent,
            ttl,
            env_var,
            command,
            args,
        ))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let detail: serde_json::Value = response.json().await.unwrap_or_default();
        anyhow::bail!(
            "failed to run command for secret '{}': {} ({})",
            secret_name,
            status,
            detail["error"]
        );
    }
    let body: serde_json::Value = response.json().await?;
    println!("Exit code: {}", body["exit_code"]);
    if let Some(lease) = body["lease_token"].as_str() {
        println!("Lease token: {lease}");
    }
    println!("Duration: {} ms", body["duration_ms"]);
    // Agents rely on the exit status: a failing child must fail this command too.
    match body["exit_code"].as_i64() {
        Some(0) => Ok(()),
        Some(code) => anyhow::bail!("command exited with status {code}"),
        None => anyhow::bail!("command did not report an exit status (killed or timed out)"),
    }
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
        SecretsCommand::Exec {
            secret_name,
            agent,
            ttl,
            argv,
        } => {
            // Clap guarantees at least the command itself, but the handler
            // fails closed rather than trusting that invariant.
            let Some((command, args)) = argv.split_first() else {
                anyhow::bail!("exec requires a command after --");
            };
            exec_with_secret(
                &secret_name,
                &agent,
                ttl,
                DEFAULT_EXEC_ENV_VAR,
                command,
                args,
            )
            .await
        }
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
    fn test_exec_command_sends_argv_array_not_a_shell_string() {
        let args = vec!["--flag".to_string(), "value with spaces".to_string()];

        let body = exec_request_body(
            "CANARY_EXEC_SECRET",
            "CANARY_EXEC_AGENT",
            30,
            DEFAULT_EXEC_ENV_VAR,
            "/bin/echo",
            &args,
        );
        let object = body.as_object().expect("body is a JSON object");

        assert_eq!(
            object["args"],
            serde_json::json!(["--flag", "value with spaces"])
        );
        assert_eq!(object["command"], serde_json::json!("/bin/echo"));
        for (field, value) in object {
            if field == "args" {
                assert!(value.is_array(), "args must be a JSON array");
                continue;
            }
            let rendered = value.as_str().unwrap_or_default();
            assert!(
                !rendered.contains("value with spaces"),
                "field '{field}' must not carry an argument: {rendered}"
            );
            assert!(
                !(rendered.contains("/bin/echo") && rendered.contains("--flag")),
                "field '{field}' concatenates command and args: {rendered}"
            );
        }
    }

    fn parse_exec(args: &[&str]) -> (String, String, Vec<String>) {
        use crate::cli::state::Cli;
        use clap::Parser;
        match Cli::try_parse_from(args).expect("exec parses").cmd {
            Some(crate::cli::commands::enums::Command::Secrets {
                cmd:
                    SecretsCommand::Exec {
                        secret_name,
                        agent,
                        argv,
                        ..
                    },
            }) => (secret_name, agent, argv),
            _ => panic!("expected the exec subcommand"),
        }
    }

    #[test]
    fn test_exec_parses_trailing_argv_after_separator() {
        use crate::cli::state::Cli;
        use clap::Parser;

        let (secret_name, agent, argv) =
            parse_exec(&["xavier", "secrets", "exec", "S", "A", "--", "/bin/true"]);
        assert_eq!(secret_name, "S");
        assert_eq!(agent, "A");
        assert_eq!(argv, vec!["/bin/true"]);

        let (secret_name, agent, argv) = parse_exec(&[
            "xavier",
            "secrets",
            "exec",
            "S",
            "A",
            "--",
            "/bin/echo",
            "--flag",
            "value with spaces",
        ]);
        assert_eq!(secret_name, "S");
        assert_eq!(agent, "A");
        assert_eq!(argv, vec!["/bin/echo", "--flag", "value with spaces"]);

        assert!(
            Cli::try_parse_from(["xavier", "secrets", "exec", "S", "A", "--"]).is_err(),
            "exec without anything after -- must be rejected"
        );
        assert!(
            Cli::try_parse_from(["xavier", "secrets", "exec", "S", "A"]).is_err(),
            "exec without -- must be rejected"
        );
    }

    #[test]
    fn test_secrets_help_lists_exec_and_hides_lend() {
        use crate::cli::state::Cli;
        use clap::CommandFactory;

        let mut app = Cli::command();
        let help = app
            .find_subcommand_mut("secrets")
            .expect("secrets subcommand exists")
            .render_help()
            .to_string();

        assert!(
            help.contains("exec"),
            "secrets --help must list exec: {help}"
        );
        assert!(
            !help
                .lines()
                .any(|line| line.trim_start().starts_with("lend ")),
            "secrets --help must not list lend: {help}"
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

    // ── `xavier keys` ─────────────────────────────────────────────────────

    #[derive(Default)]
    struct MemKeyVault {
        items: std::sync::Mutex<std::collections::HashMap<String, String>>,
    }

    impl keygen::KeyVault for MemKeyVault {
        fn store(&self, key: &str, value: &str) -> xavier::secrets::SecretResult<()> {
            self.items
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
        fn read(&self, key: &str) -> xavier::secrets::SecretResult<String> {
            self.items
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(|| xavier::secrets::SecretError::NotFound(key.to_string()))
        }
        fn remove(&self, key: &str) -> xavier::secrets::SecretResult<()> {
            self.items
                .lock()
                .unwrap()
                .remove(key)
                .map(|_| ())
                .ok_or_else(|| xavier::secrets::SecretError::NotFound(key.to_string()))
        }
    }

    #[test]
    fn keys_gen_cli_output_never_leaks_the_value() {
        let vault = MemKeyVault::default();
        let mut out = Vec::new();

        keys_generate_with_vault(&vault, "swal-vault", KeyScope::Live, 3600, &mut out)
            .expect("generate");

        let rendered = String::from_utf8(out).expect("valid utf-8");
        let stored = keygen::export_key(&vault, "swal-vault").expect("export back");

        assert!(
            !rendered.contains(&stored),
            "generate output must NOT contain the key; got: {rendered}"
        );
        assert!(rendered.contains("swal-vault"), "got: {rendered}");
        assert!(rendered.contains("fingerprint=sha256:"), "got: {rendered}");
        assert!(rendered.contains("value=<hidden>"), "got: {rendered}");
        assert!(
            rendered.contains("xavier keys export --name swal-vault"),
            "output must point at the export path; got: {rendered}"
        );
    }

    #[test]
    fn keys_gen_cli_test_flag_issues_test_scoped_key() {
        let vault = MemKeyVault::default();
        let mut out = Vec::new();

        keys_generate_with_vault(&vault, "android-dev", KeyScope::Test, 60, &mut out)
            .expect("generate");

        let rendered = String::from_utf8(out).expect("valid utf-8");
        let stored = keygen::export_key(&vault, "android-dev").expect("export back");
        assert!(stored.starts_with("xavier_test_"), "got: {stored}");
        assert!(rendered.contains("env=test"), "got: {rendered}");
    }

    #[test]
    fn keys_gen_cli_export_requires_explicit_confirmation() {
        let vault = MemKeyVault::default();
        let mut out = Vec::new();
        keys_generate_with_vault(&vault, "guarded", KeyScope::Live, 60, &mut out).expect("gen");
        let stored = keygen::export_key(&vault, "guarded").expect("export back");

        // Non-TTY without --yes: must fail closed and print nothing.
        let mut refused = Vec::new();
        let err = keys_export_with_vault(&vault, "guarded", None, false, &mut refused)
            .expect_err("non-tty export must be refused");
        assert!(
            err.to_string().contains("--yes"),
            "error must mention --yes; got: {err}"
        );
        assert!(
            !String::from_utf8(refused).expect("utf-8").contains(&stored),
            "refused export must write nothing"
        );

        // Explicit negative confirmation: also fails closed.
        let mut declined = Vec::new();
        assert!(
            keys_export_with_vault(&vault, "guarded", Some(false), true, &mut declined).is_err()
        );
        assert!(declined.is_empty(), "declined export must write nothing");

        // Explicit affirmative: this is the one path that prints the value.
        let mut allowed = Vec::new();
        keys_export_with_vault(&vault, "guarded", Some(true), true, &mut allowed).expect("export");
        assert_eq!(String::from_utf8(allowed).expect("utf-8").trim(), stored);
    }

    #[test]
    fn keys_gen_cli_list_and_revoke_round_trip() {
        let vault = MemKeyVault::default();
        let mut out = Vec::new();
        keys_generate_with_vault(&vault, "rot", KeyScope::Live, 60, &mut out).expect("gen");
        keys_generate_with_vault(&vault, "rot", KeyScope::Live, 60, &mut out).expect("rotate");

        let mut list = Vec::new();
        keys_list_with_vault(&vault, "rot", &mut list).expect("list");
        let rendered = String::from_utf8(list).expect("utf-8");
        assert!(rendered.contains("rotation=1"), "got: {rendered}");
        assert!(rendered.contains("value=<hidden>"), "got: {rendered}");

        let stored = keygen::export_key(&vault, "rot").expect("export");
        assert!(
            !rendered.contains(&stored),
            "list leaked the key: {rendered}"
        );

        let mut revoked = Vec::new();
        keys_revoke_with_vault(&vault, "rot", &mut revoked).expect("revoke");
        assert!(String::from_utf8(revoked)
            .expect("utf-8")
            .contains("revoked"));
        assert!(keygen::export_key(&vault, "rot").is_err());
        assert!(keys_list_with_vault(&vault, "rot", &mut Vec::new()).is_err());
    }

    #[test]
    fn keys_gen_cli_rejects_traversal_name() {
        let vault = MemKeyVault::default();
        let mut out = Vec::new();
        assert!(
            keys_generate_with_vault(&vault, "../escape", KeyScope::Live, 60, &mut out).is_err()
        );
        assert!(out.is_empty(), "nothing may be written on failure");
    }

    fn parse_keys(args: &[&str]) -> KeysCommand {
        use crate::cli::state::Cli;
        use clap::Parser;
        match Cli::try_parse_from(args).expect("keys parses").cmd {
            Some(crate::cli::commands::enums::Command::Keys { cmd }) => cmd,
            _ => panic!("expected the keys subcommand"),
        }
    }

    #[test]
    fn keys_gen_cli_parses_generate_flags() {
        match parse_keys(&[
            "xavier",
            "keys",
            "generate",
            "--name",
            "n",
            "--ttl-secs",
            "120",
            "--test",
        ]) {
            KeysCommand::Generate {
                name,
                ttl_secs,
                test,
            } => {
                assert_eq!(name, "n");
                assert_eq!(ttl_secs, Some(120));
                assert!(test);
            }
            other => panic!("expected generate, got {other:?}"),
        }

        match parse_keys(&["xavier", "keys", "generate", "--name", "n"]) {
            KeysCommand::Generate { ttl_secs, test, .. } => {
                assert_eq!(ttl_secs, None);
                assert!(!test);
            }
            other => panic!("expected generate, got {other:?}"),
        }

        // `--name` is required: a positional name must not be accepted.
        assert!(std::panic::catch_unwind(|| {
            parse_keys(&["xavier", "keys", "generate", "positional"])
        })
        .is_err());
    }

    #[test]
    fn keys_gen_cli_resolve_ttl_precedence() {
        // Explicit flag wins.
        assert_eq!(resolve_ttl(Some(45)).expect("ttl"), 45);
        // Absent flag falls back to the env default (90 days).
        assert_eq!(resolve_ttl(None).expect("ttl"), keygen::DEFAULT_TTL_SECS);
        // An explicit 0 means "use the default", not "never expires".
        assert_eq!(resolve_ttl(Some(0)).expect("ttl"), keygen::DEFAULT_TTL_SECS);
    }
}
