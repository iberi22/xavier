//! MCP secret tools: `secret_lend` and `secret_exec`.
//!
//! Both tools resolve their value through an injected [`SecretSource`] and lend
//! it from the process-wide shared engine ([`shared_secret_engine`]). There is
//! no `std::env` path: a vault miss is an error, so removing a variable from the
//! environment is what proves the migration.
//!
//! The vault is a parameter instead of a process-global override: production
//! wires [`HardwareVault::new`], tests wire a tempdir-scoped vault, so a test
//! never reaches the real keyring and concurrent tests never share a vault.

use std::io::Read;
use std::sync::{Arc, OnceLock};

use serde_json::{json, Value};

use super::types::*;
use crate::coordination::KeyLendingEngine;
use crate::secrets::audit::QmdAuditLogger;
use crate::secrets::exec::{run_with_secret, ExecSecretError, SecretExecSpec};
use crate::secrets::local_vault::LocalSecretsVault;
use crate::secrets::vault::HardwareVault;
use crate::secrets::{SecretError, SecretResult};

/// Service name of the production secrets vault.
pub const PRODUCTION_VAULT_SERVICE: &str = "xavier";

static SHARED_ENGINE: OnceLock<Arc<KeyLendingEngine>> = OnceLock::new();

/// The process-wide shared lending engine.
///
/// A throwaway engine is dropped with the call that created it, so the revoke
/// performed on completion would target a lease nobody can observe. Every
/// secret consumer therefore borrows from this one engine.
pub fn shared_secret_engine() -> Arc<KeyLendingEngine> {
    SHARED_ENGINE
        .get_or_init(|| {
            let audit = Box::new(QmdAuditLogger::new());
            Arc::new(KeyLendingEngine::new(audit, None))
        })
        .clone()
}

/// Read seam for secret resolution. A miss is an error: no `std::env` and no
/// `.env` fallback, by design.
pub trait SecretSource: Send + Sync {
    /// Resolve the value stored under `name`.
    fn resolve_secret(&self, name: &str) -> SecretResult<String>;
}

impl SecretSource for HardwareVault {
    fn resolve_secret(&self, name: &str) -> SecretResult<String> {
        self.get_secret(name)
    }
}

/// Integration tests compile as a separate crate and cannot reach the
/// `#[cfg(test)]`-gated [`HardwareVault::isolated`] seam, so they inject this
/// path-scoped vault instead: encrypted files under a test tempdir, synthetic
/// key, no keyring and no `~/.xavier` involved.
impl SecretSource for LocalSecretsVault {
    fn resolve_secret(&self, name: &str) -> SecretResult<String> {
        self.get(name)
            .map_err(|e| SecretError::ProviderError(e.to_string()))
    }
}

/// Dependencies of the MCP secret tools.
#[derive(Clone)]
pub struct SecretToolContext {
    engine: Arc<KeyLendingEngine>,
    vault: Arc<HardwareVault>,
}

impl SecretToolContext {
    /// Shared engine and the real `xavier` vault.
    pub fn production() -> Self {
        Self {
            engine: shared_secret_engine(),
            vault: Arc::new(HardwareVault::new(PRODUCTION_VAULT_SERVICE)),
        }
    }

    /// Injected vault, e.g. a tempdir vault in tests. The engine stays shared,
    /// so leases remain globally observable.
    pub fn with_vault(vault: HardwareVault) -> Self {
        Self {
            engine: shared_secret_engine(),
            vault: Arc::new(vault),
        }
    }

    /// The injected vault as a resolution seam.
    pub fn source(&self) -> &dyn SecretSource {
        self.vault.as_ref()
    }
}

/// Borrow a secret from `source` for the duration of `f`: lends it under a
/// short TTL (registering it with the leak detector), hands it to the callback
/// and revokes the lease on completion. No value ever reaches the returned
/// error, the audit log or a tracing event.
pub async fn resolve_tool_secret<F, Fut, T>(
    source: &dyn SecretSource,
    secret_name: &str,
    agent_id: &str,
    f: F,
) -> anyhow::Result<T>
where
    F: FnOnce(Option<String>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let engine = shared_secret_engine();
    let secret_val = source
        .resolve_secret(secret_name)
        .map_err(|e| anyhow::anyhow!("Failed to resolve secret '{secret_name}' from vault: {e}"))?;

    let lease = engine
        .lend(secret_name, Some(&secret_val), agent_id, 60)
        .await?;

    let val = lease.secret_value.clone();
    let res = f(val).await;

    let _ = engine
        .revoke(&lease.token, "mcp_tool_execution_complete")
        .await;

    res
}

/// Tools exposed by the secrets module.
pub fn get_xavier_secrets_tools() -> Vec<MCPTool> {
    vec![
        MCPTool {
            name: "secret_lend".to_string(),
            description:
                "Lend a secret lease from the vault and return token and metadata (never value)"
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "secret_name": { "type": "string", "description": "Vault key name of the secret" },
                    "agent_id": { "type": "string", "description": "Identifier of the requesting agent" },
                    "ttl_seconds": { "type": "integer", "description": "Lease duration in seconds (default: 60)" }
                },
                "required": ["secret_name", "agent_id"]
            }),
        },
        MCPTool {
            name: "secret_exec".to_string(),
            description: "Execute a command with an ephemeral secret injected into its environment"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "secret_name": { "type": "string", "description": "Vault key name of the secret" },
                    "command": { "type": "string", "description": "Executable program to run" },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments passed to the command"
                    },
                    "env_var": { "type": "string", "description": "Environment variable name to inject secret into (default: XAVIER_SECRET)" },
                    "ttl_seconds": { "type": "integer", "description": "Execution timeout in seconds (default: 3600)" },
                    "agent_id": { "type": "string", "description": "Agent identifier for audit and lease (default: mcp_secret_exec)" }
                },
                "required": ["secret_name", "command"]
            }),
        },
    ]
}

/// Check if tool name belongs to secrets module.
pub fn is_secrets_tool(name: &str) -> bool {
    matches!(name, "secret_lend" | "secret_exec")
}

fn required_str<'a>(arguments: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    arguments
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required '{key}' argument"))
}

fn optional_str<'a>(arguments: &'a Value, key: &str, default: &'a str) -> &'a str {
    arguments
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
}

fn optional_u64(arguments: &Value, key: &str, default: u64) -> u64 {
    arguments
        .get(key)
        .and_then(|v| v.as_u64())
        .unwrap_or(default)
}

fn string_args(arguments: &Value, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|val| val.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Maximum returned output size for secret_exec (64 KiB).
pub const MAX_SECRET_EXEC_OUTPUT_BYTES: usize = 64 * 1024;
const _: () = assert!(MAX_SECRET_EXEC_OUTPUT_BYTES == 65536);

/// Truncation marker appended when child output exceeds [`MAX_SECRET_EXEC_OUTPUT_BYTES`].
pub const TRUNCATION_MARKER: &str = "\n[TRUNCATED]";

/// Hard cap on bytes loaded into memory from the child's stdout file,
/// independent of how much the child actually wrote. Twice the returned-output
/// cap leaves room for a secret occurrence straddling the truncation boundary
/// in [`redact_and_truncate`] while still bounding memory for a runaway child.
const MAX_STDOUT_READ_BYTES: usize = MAX_SECRET_EXEC_OUTPUT_BYTES * 2;

/// Temporary stdout capture file with restrictive `0600` permissions, removed on drop.
pub(crate) struct TempStdoutGuard {
    path: std::path::PathBuf,
}

impl TempStdoutGuard {
    /// Create a new temporary stdout file with 0600 permissions.
    pub(crate) fn new() -> std::io::Result<Self> {
        let temp_dir = std::env::temp_dir();
        let filename = format!("xavier_exec_stdout_{}.tmp", uuid::Uuid::new_v4());
        let path = temp_dir.join(filename);

        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path)?;
        drop(file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(Self { path })
    }

    /// Path to the temporary file.
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Read the captured output (capped at [`MAX_STDOUT_READ_BYTES`]) and
    /// delete the file immediately.
    pub(crate) fn read_and_delete(&mut self) -> String {
        let content = std::fs::File::open(&self.path)
            .and_then(|file| {
                let mut buf = Vec::new();
                file.take(MAX_STDOUT_READ_BYTES as u64)
                    .read_to_end(&mut buf)?;
                Ok(buf)
            })
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let _ = std::fs::remove_file(&self.path);
        content
    }
}

impl Drop for TempStdoutGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Redact occurrences of `secret_val` and truncate to [`MAX_SECRET_EXEC_OUTPUT_BYTES`].
pub(crate) fn redact_and_truncate(mut text: String, secret_val: Option<&str>) -> (String, bool) {
    if let Some(secret) = secret_val {
        if !secret.is_empty() {
            text = text.replace(secret, "[REDACTED]");
        }
    }

    if text.len() > MAX_SECRET_EXEC_OUTPUT_BYTES {
        let max_len = MAX_SECRET_EXEC_OUTPUT_BYTES.saturating_sub(TRUNCATION_MARKER.len());
        let mut cut = max_len;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let mut truncated_text = text[..cut].to_string();
        truncated_text.push_str(TRUNCATION_MARKER);
        (truncated_text, true)
    } else {
        (text, false)
    }
}

/// Dispatch a secrets tool call.
pub async fn handle_secrets_tool(
    ctx: &SecretToolContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match name {
        "secret_lend" => {
            let secret_name = required_str(&arguments, "secret_name")?;
            let agent_id = required_str(&arguments, "agent_id")?;
            let ttl_seconds = optional_u64(&arguments, "ttl_seconds", 60);

            let secret_val = ctx.vault.resolve_secret(secret_name).map_err(|e| {
                anyhow::anyhow!("Failed to resolve secret '{secret_name}' from vault: {e}")
            })?;

            let lease = ctx
                .engine
                .lend(secret_name, Some(&secret_val), agent_id, ttl_seconds)
                .await?;

            let payload = json!({
                "lease_token": lease.token,
                "secret_name": lease.secret_name,
                "agent_id": lease.agent_id,
                "expires_at": lease.expires_at.to_rfc3339(),
                "revoked": false
            });

            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, false,
            ))?)
        }
        "secret_exec" => {
            let secret_name = required_str(&arguments, "secret_name")?;
            let command = required_str(&arguments, "command")?;
            let args = string_args(&arguments, "args");
            let env_var = optional_str(&arguments, "env_var", "XAVIER_SECRET");
            let ttl_seconds = optional_u64(&arguments, "ttl_seconds", 3600);
            let agent_id = optional_str(&arguments, "agent_id", "mcp_secret_exec");

            // Resolved before the child runs, not after: `run_with_secret`
            // deliberately never returns the value it injected, and reading it
            // back from the vault only after the child exits would redact
            // against whatever value is current then. If the secret rotates
            // during the run (TTL defaults to an hour) that mismatches what the
            // child actually saw, letting the real value slip through
            // unredacted. Resolving up front narrows that window to the gap
            // between this read and `run_with_secret`'s own internal read.
            let secret_val = ctx.vault.resolve_secret(secret_name).ok();

            let mut stdout_guard = TempStdoutGuard::new()?;
            let spec =
                SecretExecSpec::new(secret_name, env_var, command, args, ttl_seconds, agent_id)
                    .with_stdout_path(stdout_guard.path());

            let result = run_with_secret(&spec, Arc::clone(&ctx.engine), &ctx.vault).await;

            let outcome = match result {
                Ok(outcome) => outcome,
                Err(err) => {
                    drop(stdout_guard);
                    let is_timeout = matches!(err, ExecSecretError::Timeout(_));
                    let payload = json!({
                        "error": err.to_string(),
                        "lease_token": Value::Null,
                        "timed_out": is_timeout,
                        "revoked": true
                    });
                    return Ok(serde_json::to_value(MCPToolResult::structured(
                        payload, true,
                    ))?);
                }
            };

            let raw_output = stdout_guard.read_and_delete();
            let (stdout_text, truncated) = redact_and_truncate(raw_output, secret_val.as_deref());

            let payload = json!({
                "exit_code": outcome.exit_code,
                "stdout": stdout_text,
                "output": stdout_text,
                "truncated": truncated,
                "lease_token": Value::Null,
                "duration_ms": outcome.duration_ms,
                "success": outcome.success,
                "timed_out": outcome.timed_out,
                "killed": outcome.killed,
                "revoked": true
            });
            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, false,
            ))?)
        }
        _ => Err(anyhow::anyhow!("Unknown secrets tool: {name}")),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::security::auth::{Claims, UserRole};

    /// Per-test fixture: an isolated vault (no keyring, no real `~/.xavier`), a
    /// synthetic canary under a unique key, and a tempdir owning the store.
    struct Fixture {
        temp_dir: tempfile::TempDir,
        secret_name: String,
        canary: String,
        ctx: SecretToolContext,
    }

    impl Fixture {
        fn new() -> Self {
            let temp_dir = tempfile::tempdir().expect("tempdir");
            let vault =
                HardwareVault::new("x").isolated(temp_dir.path().join("secrets"), [42u8; 32]);
            let secret_name = format!("MCP_SECRET_{}", uuid::Uuid::new_v4().simple());
            let canary = format!("canary_val_{}", uuid::Uuid::new_v4().simple());
            vault.store_secret(&secret_name, &canary).expect("seed");
            let ctx = SecretToolContext::with_vault(vault);
            Self {
                temp_dir,
                secret_name,
                canary,
                ctx,
            }
        }

        /// A tool response must never carry the canary, in any rendering.
        fn assert_canary_absent(&self, response: &Value) {
            let rendered = serde_json::to_string(response).expect("serialize response");
            assert!(!rendered.contains(&self.canary), "leaked the value");
        }
    }

    #[tokio::test]
    async fn test_secret_lend_returns_token_not_value() {
        let f = Fixture::new();
        let args = json!({
            "secret_name": f.secret_name,
            "agent_id": "test_agent",
            "ttl_seconds": 60
        });

        let res = handle_secrets_tool(&f.ctx, "secret_lend", args)
            .await
            .expect("lend succeeds");
        f.assert_canary_absent(&res);

        let structured = &res["structuredContent"];
        let token = structured["lease_token"].as_str().expect("lease_token");
        assert!(!token.is_empty());
        assert_eq!(structured["secret_name"], f.secret_name);
        assert_eq!(structured["agent_id"], "test_agent");
        assert_eq!(structured["revoked"], false);
    }

    #[tokio::test]
    async fn test_secret_exec_returns_exit_code_not_value() {
        let f = Fixture::new();
        let args = json!({
            "secret_name": f.secret_name,
            "command": "true",
            "args": []
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("exec succeeds");
        f.assert_canary_absent(&res);

        let structured = &res["structuredContent"];
        assert_eq!(structured["exit_code"], 0);
        assert_eq!(structured["success"], true);
        assert_eq!(structured["revoked"], true);
    }

    #[test]
    fn test_temp_stdout_file_permissions_0600() {
        let guard = TempStdoutGuard::new().expect("create temp stdout guard");
        assert!(guard.path().exists(), "temp file must exist after creation");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(guard.path()).expect("metadata");
            assert_eq!(
                meta.permissions().mode() & 0o777,
                0o600,
                "temporary stdout file must be created with 0600 permissions"
            );
        }

        let path = guard.path().to_path_buf();
        drop(guard);
        assert!(
            !path.exists(),
            "temporary stdout file must be removed on drop"
        );
    }

    #[test]
    fn test_redact_and_truncate_caps_at_64kib_with_marker() {
        let canary = "SECRET_CANARY_VALUE_12345";
        let prefix = "A".repeat(70_000);
        let text = format!("{prefix}_{canary}_suffix");

        let (redacted, truncated) = redact_and_truncate(text, Some(canary));
        assert!(truncated, "text longer than 64 KiB must be truncated");
        assert!(
            !redacted.contains(canary),
            "canary must not appear in output"
        );
        assert!(
            redacted.len() <= MAX_SECRET_EXEC_OUTPUT_BYTES,
            "redacted len {} must be <= {}",
            redacted.len(),
            MAX_SECRET_EXEC_OUTPUT_BYTES
        );
        assert!(
            redacted.ends_with(TRUNCATION_MARKER),
            "redacted output must end with explicit truncation marker"
        );
    }

    #[tokio::test]
    async fn test_secret_exec_redacts_secret_in_stdout() {
        let f = Fixture::new();
        let args = json!({
            "secret_name": f.secret_name,
            "command": "sh",
            "args": ["-c", "echo hello_${XAVIER_SECRET}_world"]
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("exec succeeds");
        f.assert_canary_absent(&res);

        let structured = &res["structuredContent"];
        assert_eq!(structured["exit_code"], 0);
        assert_eq!(structured["success"], true);
        assert_eq!(structured["revoked"], true);

        let stdout = structured["stdout"].as_str().expect("stdout present");
        assert!(
            stdout.contains("hello_[REDACTED]_world"),
            "expected stdout to contain redacted secret, got: {stdout}"
        );
    }

    #[tokio::test]
    async fn test_secret_exec_temp_file_removed_on_success() {
        let f = Fixture::new();
        let args = json!({
            "secret_name": f.secret_name,
            "command": "sh",
            "args": ["-c", "readlink /proc/self/fd/1; echo secret_$XAVIER_SECRET"]
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("exec succeeds");
        f.assert_canary_absent(&res);

        let stdout = res["structuredContent"]["stdout"]
            .as_str()
            .expect("stdout string");
        let first_line = stdout
            .lines()
            .next()
            .expect("first line has temp file path");
        assert!(
            first_line.contains("xavier_exec_stdout_"),
            "expected temp file path, got: {first_line}"
        );
        let temp_path = std::path::Path::new(first_line);
        assert!(
            !temp_path.exists(),
            "temporary output file must be removed after successful execution"
        );
    }

    #[tokio::test]
    async fn test_secret_exec_temp_file_removed_on_timeout() {
        let f = Fixture::new();
        let recorded_path_file = f.temp_dir.path().join("recorded_stdout_path.txt");
        let script = format!(
            "exec 9>&1; readlink /proc/self/fd/9 > '{}'; sleep 5",
            recorded_path_file.display()
        );
        let args = json!({
            "secret_name": f.secret_name,
            "command": "sh",
            "args": ["-c", script],
            "ttl_seconds": 1
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("exec returns response");
        f.assert_canary_absent(&res);

        let structured = &res["structuredContent"];
        assert_eq!(structured["timed_out"], true);

        let recorded_path_str = std::fs::read_to_string(&recorded_path_file)
            .expect("child wrote stdout path before timeout");
        let temp_path = std::path::Path::new(recorded_path_str.trim());
        assert!(
            recorded_path_str.contains("xavier_exec_stdout_"),
            "expected temp file path, got: {recorded_path_str}"
        );
        assert!(
            !temp_path.exists(),
            "temporary output file must be removed on timeout"
        );
    }

    #[tokio::test]
    async fn test_secret_exec_temp_file_removed_on_nonzero_exit_status() {
        let f = Fixture::new();
        let args = json!({
            "secret_name": f.secret_name,
            "command": "sh",
            "args": ["-c", "readlink /proc/self/fd/1; exit 42"]
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("exec returns response");
        f.assert_canary_absent(&res);

        let structured = &res["structuredContent"];
        assert_eq!(structured["exit_code"], 42);
        assert_eq!(structured["success"], false);

        let stdout = structured["stdout"].as_str().expect("stdout string");
        let first_line = stdout
            .lines()
            .next()
            .expect("first line has temp file path");
        assert!(
            first_line.contains("xavier_exec_stdout_"),
            "expected temp file path, got: {first_line}"
        );
        let temp_path = std::path::Path::new(first_line);
        assert!(
            !temp_path.exists(),
            "temporary output file must be removed after a nonzero exit status"
        );
    }

    /// Number of leftover `xavier_exec_stdout_*` files sitting in the system
    /// temp dir, used to prove cleanup on a path that never spawns a child (so
    /// there is no child-printed path to assert against directly).
    fn count_temp_stdout_files() -> usize {
        std::fs::read_dir(std::env::temp_dir())
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_str()
                            .map(|name| name.starts_with("xavier_exec_stdout_"))
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    /// Exercises the genuine `Err(err) => { drop(stdout_guard); ... }` branch of
    /// `handle_secrets_tool`: a secret name absent from the vault fails inside
    /// `run_with_secret` before any child ever spawns, so this is not the
    /// success-with-nonzero-exit path covered above.
    #[tokio::test]
    async fn test_secret_exec_temp_file_removed_on_error() {
        let f = Fixture::new();
        let before = count_temp_stdout_files();
        let args = json!({
            "secret_name": "MCP_SECRET_ABSENT_FROM_VAULT_FOR_ERROR_TEST",
            "command": "true",
            "args": []
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("exec returns a response even when the vault lookup fails");
        f.assert_canary_absent(&res);

        let structured = &res["structuredContent"];
        assert!(
            structured["error"].as_str().is_some(),
            "expected the vault-lookup failure surfaced as an error"
        );
        assert_eq!(structured["revoked"], true);

        assert_eq!(
            count_temp_stdout_files(),
            before,
            "temporary stdout file must be removed on the genuine Err(...) exit path"
        );
    }

    #[tokio::test]
    async fn test_secret_exec_output_truncated_at_64kib() {
        let f = Fixture::new();
        let args = json!({
            "secret_name": f.secret_name,
            "command": "sh",
            "args": ["-c", "head -c 70000 /dev/zero | tr '\\0' 'A'"]
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("exec succeeds");
        f.assert_canary_absent(&res);

        let structured = &res["structuredContent"];
        assert_eq!(structured["exit_code"], 0);
        assert_eq!(structured["truncated"], true);

        let stdout = structured["stdout"].as_str().expect("stdout string");
        assert!(
            stdout.len() <= MAX_SECRET_EXEC_OUTPUT_BYTES,
            "stdout len {} must be <= {}",
            stdout.len(),
            MAX_SECRET_EXEC_OUTPUT_BYTES
        );
        assert!(
            stdout.ends_with(TRUNCATION_MARKER),
            "stdout must end with explicit truncation marker, got end: {:?}",
            &stdout[stdout.len().saturating_sub(50)..]
        );
    }

    #[tokio::test]
    async fn test_secret_lend_appears_in_shared_engine_list_leases() {
        let f = Fixture::new();
        let agent_id = format!("lease_agent_{}", uuid::Uuid::new_v4().simple());
        let args = json!({
            "secret_name": f.secret_name,
            "agent_id": agent_id,
            "ttl_seconds": 300
        });

        let res = handle_secrets_tool(&f.ctx, "secret_lend", args)
            .await
            .expect("lend succeeds");
        let token = res["structuredContent"]["lease_token"]
            .as_str()
            .expect("lease_token");

        let engine = shared_secret_engine();
        let leases = engine.list_leases().await;
        assert!(
            leases
                .iter()
                .any(|l| l.token == token && l.agent_id == agent_id),
            "Lease token must appear in the shared engine's list_leases"
        );

        let _ = engine.revoke(token, "test_cleanup").await;
    }

    /// A lease created through the tool must be revocable via the shared engine.
    #[tokio::test]
    async fn test_resolve_tool_secret_revokes_on_shared_engine() {
        let f = Fixture::new();
        let agent_id = format!("resolve_agent_{}", uuid::Uuid::new_v4().simple());

        let seen = resolve_tool_secret(
            f.ctx.source(),
            &f.secret_name,
            &agent_id,
            |secret_opt| async move {
                let val = secret_opt.expect("lease carries the value");
                Ok::<_, anyhow::Error>(val)
            },
        )
        .await
        .expect("resolve succeeds");
        assert_eq!(seen, f.canary);

        let engine = shared_secret_engine();
        let active = engine.list_leases().await;
        assert!(
            !active.iter().any(|l| l.agent_id == agent_id),
            "Lease must be revoked once the callback returns"
        );
    }

    /// A vault miss is an error: no `std::env` fallback, and the callback never
    /// runs. The flag is set inside the closure and checked after the call, so a
    /// resolver that wrongly ran the callback would fail here.
    #[tokio::test]
    async fn test_resolve_tool_secret_fails_closed_on_vault_miss() {
        let f = Fixture::new();
        let mut callback_ran = false;

        let result = resolve_tool_secret(
            f.ctx.source(),
            "MCP_SECRET_ABSENT",
            "miss_agent",
            |_secret_opt| {
                callback_ran = true;
                async { Ok::<_, anyhow::Error>(()) }
            },
        )
        .await;

        assert!(result.is_err(), "A vault miss must be an error");
        assert!(!callback_ran, "The callback must not run without a lease");
    }

    /// Assert `name` is denied for a lesser JWT role and for an anonymous
    /// caller. The secret does not exist, so a dispatch that slipped past the
    /// gate would fail on the vault lookup rather than succeed.
    async fn assert_denied(name: &str) {
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let args = json!({
            "secret_name": "MCP_SECRET_ABSENT",
            "agent_id": "unprivileged_agent",
            "command": "true"
        });

        let user_claims = Claims::new(
            "user1".to_string(),
            "user@swal.dev".to_string(),
            UserRole::User,
            chrono::Duration::hours(1),
        );

        let res_user = crate::server::mcp::server::handle_tool_call(
            state.clone(),
            workspace.clone(),
            Some(&user_claims),
            name,
            args.clone(),
        )
        .await;
        assert!(
            res_user.is_err(),
            "{name} should be denied for the User role"
        );
        assert!(res_user
            .expect_err("user call is denied")
            .to_string()
            .contains("Forbidden: Insufficient permissions"));

        let res_anon =
            crate::server::mcp::server::handle_tool_call(state, workspace, None, name, args).await;
        assert!(
            res_anon.is_err(),
            "{name} should be denied for an anonymous caller"
        );
        assert!(res_anon
            .expect_err("anonymous call is denied")
            .to_string()
            .contains("Forbidden: Insufficient permissions"));
    }

    #[tokio::test]
    async fn test_secret_lend_denied_for_unprivileged_role() {
        assert_denied("secret_lend").await;
    }

    #[tokio::test]
    async fn test_secret_exec_denied_for_unprivileged_role() {
        assert_denied("secret_exec").await;
    }

    #[tokio::test]
    async fn test_tools_list_includes_secret_tools_with_schemas() {
        let tools = crate::server::mcp::server::get_xavier_tools();
        let secret_tools: Vec<_> = tools
            .iter()
            .filter(|tool| super::is_secrets_tool(&tool.name))
            .collect();

        let names: Vec<&str> = secret_tools.iter().map(|t| t.name.as_str()).collect();
        assert!(
            names.contains(&"secret_lend"),
            "tools/list must include secret_lend"
        );
        assert!(
            names.contains(&"secret_exec"),
            "tools/list must include secret_exec"
        );

        for tool in &secret_tools {
            let schema = &tool.input_schema;
            assert_eq!(
                schema["type"], "object",
                "{} must declare an object input schema",
                tool.name
            );
            assert!(
                schema["properties"].is_object()
                    && !schema["properties"].as_object().unwrap().is_empty(),
                "{} must declare input properties",
                tool.name
            );
            assert!(
                schema["required"].is_array() && !schema["required"].as_array().unwrap().is_empty(),
                "{} must declare required fields",
                tool.name
            );
        }
    }
}
