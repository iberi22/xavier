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
use crate::secrets::exec::{
    is_valid_env_var, run_with_secret, ExecSecretError, SecretExecSpec, DISALLOWED_ENV_VARS,
};
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

fn tool_error(msg: impl Into<String>) -> anyhow::Result<Value> {
    let payload = json!({
        "error": msg.into(),
        "lease_token": Value::Null,
        "revoked": true
    });
    Ok(serde_json::to_value(MCPToolResult::structured(
        payload, true,
    ))?)
}

fn required_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Err(format!("Missing required parameter: {key}")),
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(_) => Err(format!("Invalid argument '{key}': expected string")),
    }
}

fn optional_str<'a>(arguments: &'a Value, key: &str, default: &'a str) -> Result<&'a str, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(_) => Err(format!("Invalid argument '{key}': expected string")),
    }
}

/// Maximum TTL allowed for secrets leases (86400 seconds = 24 hours).
pub const MAX_SECRET_TTL_SECONDS: u64 = 86400;

fn optional_ttl(arguments: &Value, key: &str, default: u64) -> Result<u64, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Number(n)) => {
            if let Some(val) = n.as_u64() {
                if val < 1 {
                    Err(format!("Invalid argument '{key}': must be >= 1"))
                } else {
                    Ok(val.min(MAX_SECRET_TTL_SECONDS))
                }
            } else if let Some(val) = n.as_i64() {
                if val < 1 {
                    Err(format!("Invalid argument '{key}': must be >= 1"))
                } else {
                    Ok((val as u64).min(MAX_SECRET_TTL_SECONDS))
                }
            } else {
                Err(format!("Invalid argument '{key}': expected integer"))
            }
        }
        Some(_) => Err(format!("Invalid argument '{key}': expected integer")),
    }
}

fn string_args(arguments: &Value, key: &str) -> Result<Vec<String>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => {
            let mut result = Vec::with_capacity(arr.len());
            for item in arr {
                match item.as_str() {
                    Some(s) => result.push(s.to_string()),
                    None => {
                        return Err(format!(
                            "Invalid argument '{key}': expected array of strings"
                        ))
                    }
                }
            }
            Ok(result)
        }
        Some(_) => Err(format!(
            "Invalid argument '{key}': expected array of strings"
        )),
    }
}

fn parse_env_var<'a>(arguments: &'a Value, key: &str, default: &'a str) -> Result<&'a str, String> {
    let var = optional_str(arguments, key, default)?;
    if !is_valid_env_var(var) {
        if DISALLOWED_ENV_VARS.contains(&var) {
            return Err(format!(
                "Invalid argument '{key}': disallowed environment variable name '{var}'"
            ));
        } else {
            return Err(format!(
                "Invalid argument '{key}': must match ^[A-Za-z_][A-Za-z0-9_]*$"
            ));
        }
    }
    Ok(var)
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

fn base64_encode_custom(data: &[u8], alphabet: &[u8; 64], padded: bool) -> String {
    if data.is_empty() {
        return String::new();
    }
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        result.push(alphabet[((b0 >> 2) & 0x3f) as usize] as char);
        result.push(alphabet[(((b0 << 4) | (b1 >> 4)) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            result.push(alphabet[(((b1 << 2) | (b2 >> 6)) & 0x3f) as usize] as char);
        } else if padded {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(alphabet[(b2 & 0x3f) as usize] as char);
        } else if padded {
            result.push('=');
        }
    }
    result
}

/// Generate all encoded representations of `secret` for redaction.
pub(crate) fn secret_redaction_patterns(secret: &str) -> Vec<String> {
    if secret.is_empty() {
        return Vec::new();
    }
    const STD_ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const URL_ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let bytes = secret.as_bytes();
    let mut patterns = Vec::with_capacity(10);
    patterns.push(secret.to_string());
    patterns.push(base64_encode_custom(bytes, STD_ALPHABET, true));
    patterns.push(base64_encode_custom(bytes, STD_ALPHABET, false));
    patterns.push(base64_encode_custom(bytes, URL_ALPHABET, true));
    patterns.push(base64_encode_custom(bytes, URL_ALPHABET, false));

    let hex_lower: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let hex_upper: String = bytes.iter().map(|b| format!("{:02X}", b)).collect();
    patterns.push(hex_lower);
    patterns.push(hex_upper);

    let url_enc = urlencoding::encode(secret).to_string();
    patterns.push(url_enc.clone());
    if url_enc.contains('%') {
        let mut url_enc_lower = String::with_capacity(url_enc.len());
        let mut chars = url_enc.chars();
        while let Some(c) = chars.next() {
            if c == '%' {
                url_enc_lower.push('%');
                if let Some(h1) = chars.next() {
                    url_enc_lower.push(h1.to_ascii_lowercase());
                }
                if let Some(h2) = chars.next() {
                    url_enc_lower.push(h2.to_ascii_lowercase());
                }
            } else {
                url_enc_lower.push(c);
            }
        }
        patterns.push(url_enc_lower);
    }

    patterns.retain(|p| !p.is_empty());
    // Sort descending by length so longer substrings are matched and redacted first
    patterns.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    patterns.dedup();
    patterns
}

/// Redact occurrences of `secret_val` and its encoded forms, and truncate to [`MAX_SECRET_EXEC_OUTPUT_BYTES`].
pub(crate) fn redact_and_truncate(mut text: String, secret_val: Option<&str>) -> (String, bool) {
    if let Some(secret) = secret_val {
        if !secret.is_empty() {
            for pattern in secret_redaction_patterns(secret) {
                text = text.replace(&pattern, "[REDACTED]");
            }
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
    if !arguments.is_object() {
        return tool_error("Invalid arguments: expected JSON object");
    }

    match name {
        "secret_lend" => {
            let secret_name = match required_str(&arguments, "secret_name") {
                Ok(s) => s,
                Err(e) => return tool_error(e),
            };
            let agent_id = match required_str(&arguments, "agent_id") {
                Ok(s) => s,
                Err(e) => return tool_error(e),
            };
            let ttl_seconds = match optional_ttl(&arguments, "ttl_seconds", 60) {
                Ok(t) => t,
                Err(e) => return tool_error(e),
            };

            let secret_val = match ctx.vault.resolve_secret(secret_name) {
                Ok(val) => val,
                Err(e) => {
                    return tool_error(format!(
                        "Failed to resolve secret '{secret_name}' from vault: {e}"
                    ));
                }
            };

            let lease = match ctx
                .engine
                .lend(secret_name, Some(&secret_val), agent_id, ttl_seconds)
                .await
            {
                Ok(l) => l,
                Err(e) => {
                    return tool_error(format!("Failed to lend secret '{secret_name}': {e}"));
                }
            };

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
            let secret_name = match required_str(&arguments, "secret_name") {
                Ok(s) => s,
                Err(e) => return tool_error(e),
            };
            let command = match required_str(&arguments, "command") {
                Ok(s) => s,
                Err(e) => return tool_error(e),
            };
            let args = match string_args(&arguments, "args") {
                Ok(a) => a,
                Err(e) => return tool_error(e),
            };
            let env_var = match parse_env_var(&arguments, "env_var", "XAVIER_SECRET") {
                Ok(v) => v,
                Err(e) => return tool_error(e),
            };
            let ttl_seconds = match optional_ttl(&arguments, "ttl_seconds", 3600) {
                Ok(t) => t,
                Err(e) => return tool_error(e),
            };
            let agent_id = match optional_str(&arguments, "agent_id", "mcp_secret_exec") {
                Ok(a) => a,
                Err(e) => return tool_error(e),
            };

            let secret_val = match ctx.vault.resolve_secret(secret_name) {
                Ok(val) => val,
                Err(e) => {
                    let payload = json!({
                        "error": format!("Vault error: {e}"),
                        "lease_token": Value::Null,
                        "timed_out": false,
                        "revoked": true
                    });
                    return Ok(serde_json::to_value(MCPToolResult::structured(
                        payload, true,
                    ))?);
                }
            };

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
            let (stdout_text, truncated) = redact_and_truncate(raw_output, Some(&secret_val));

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

    /// Number of leftover `xavier_exec_stdout_*` files sitting in `dir`, used
    /// to prove cleanup on a path that never spawns a child (so there is no
    /// child-printed path to assert against directly).
    ///
    /// The dir is a parameter, never `std::env::temp_dir()`: the exec path
    /// resolves the system temp dir, which nextest processes share, so sibling
    /// secret_exec tests momentarily own such files there and the before/after
    /// delta raced (K1 nextest flake).
    fn count_temp_stdout_files(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir)
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

    /// A private temp dir plus a `TMPDIR` override pointing at it, so the exec
    /// path under test and [`count_temp_stdout_files`] agree on a directory no
    /// other test process can touch. Restores the previous `TMPDIR` on drop.
    struct PrivateTempDir {
        previous: Option<std::ffi::OsString>,
        dir: tempfile::TempDir,
    }

    impl PrivateTempDir {
        fn new() -> Self {
            // Created BEFORE the override, so the override cannot decide where
            // the sandbox itself lands.
            let dir = tempfile::tempdir().expect("private temp dir for exec stdout files");
            let previous = std::env::var_os("TMPDIR");
            std::env::set_var("TMPDIR", dir.path());
            Self { previous, dir }
        }

        fn path(&self) -> &std::path::Path {
            self.dir.path()
        }
    }

    impl Drop for PrivateTempDir {
        fn drop(&mut self) {
            match &self.previous {
                Some(p) => std::env::set_var("TMPDIR", p),
                None => std::env::remove_var("TMPDIR"),
            }
        }
    }

    /// Exercises the genuine `Err(err) => { drop(stdout_guard); ... }` branch of
    /// `handle_secrets_tool`: a secret name absent from the vault fails inside
    /// `run_with_secret` before any child ever spawns, so this is not the
    /// success-with-nonzero-exit path covered above.
    #[tokio::test]
    async fn test_secret_exec_temp_file_removed_on_error() {
        let f = Fixture::new();
        let tmp = PrivateTempDir::new();
        let before = count_temp_stdout_files(tmp.path());
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
            count_temp_stdout_files(tmp.path()),
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

    #[tokio::test]
    async fn secret_exec_fails_closed_when_secret_unavailable() {
        let f = Fixture::new();
        let non_existent_tmp = f.temp_dir.path().join("unreachable_tmpdir");
        // Restores TMPDIR even if an assertion below panics.
        struct TmpDirGuard(Option<std::ffi::OsString>);
        impl Drop for TmpDirGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(prev) => std::env::set_var("TMPDIR", prev),
                    None => std::env::remove_var("TMPDIR"),
                }
            }
        }
        let _tmp_guard = TmpDirGuard(std::env::var_os("TMPDIR"));
        std::env::set_var("TMPDIR", &non_existent_tmp);

        let args = json!({
            "secret_name": "MCP_SECRET_DEFINITELY_ABSENT",
            "command": "echo",
            "args": ["harmless"]
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("must return tool error response without erroring on tempfile creation");

        assert_eq!(res["isError"], true);
        let structured = &res["structuredContent"];
        assert_eq!(structured["revoked"], true);
        assert_eq!(structured["lease_token"], Value::Null);
        let err_msg = structured["error"].as_str().expect("error message");
        assert!(err_msg.contains("Vault error"));
    }

    #[tokio::test]
    async fn secret_exec_redacts_encoded_forms() {
        let f = Fixture::new();
        let secret_val = "sec>~??_";
        let secret_name = format!("MCP_SECRET_ENC_{}", uuid::Uuid::new_v4().simple());
        f.ctx
            .vault
            .store_secret(&secret_name, secret_val)
            .expect("seed canary");

        let hex_lower = "7365633e7e3f3f5f";
        let hex_upper = "7365633E7E3F3F5F";
        let b64_std_padded = "c2VjPn4/P18=";
        let b64_std_unpadded = "c2VjPn4/P18";
        let b64_url_padded = "c2VjPn4_P18=";
        let b64_url_unpadded = "c2VjPn4_P18";
        let url_encoded = "sec%3E~%3F%3F_";
        let url_encoded_lower = "sec%3e~%3f%3f_";

        let args = json!({
            "secret_name": secret_name,
            "command": "printf",
            "args": [
                "%s
%s
%s
%s
%s
%s
%s
%s
%s
",
                secret_val,
                hex_lower,
                hex_upper,
                b64_std_padded,
                b64_std_unpadded,
                b64_url_padded,
                b64_url_unpadded,
                url_encoded,
                url_encoded_lower
            ]
        });

        let res = handle_secrets_tool(&f.ctx, "secret_exec", args)
            .await
            .expect("exec succeeds");
        assert_eq!(res["isError"], false);

        let stdout = res["structuredContent"]["stdout"]
            .as_str()
            .expect("stdout present");
        assert!(
            !stdout.contains(secret_val),
            "exact secret must be redacted"
        );
        assert!(
            !stdout.contains(hex_lower),
            "lowercase hex must be redacted"
        );
        assert!(
            !stdout.contains(hex_upper),
            "uppercase hex must be redacted"
        );
        assert!(
            !stdout.contains(b64_std_padded),
            "standard base64 must be redacted"
        );
        assert!(
            !stdout.contains(b64_std_unpadded),
            "unpadded standard base64 must be redacted"
        );
        assert!(
            !stdout.contains(b64_url_padded),
            "url-safe base64 must be redacted"
        );
        assert!(
            !stdout.contains(b64_url_unpadded),
            "unpadded url-safe base64 must be redacted"
        );
        assert!(
            !stdout.contains(url_encoded),
            "url-encoded form must be redacted"
        );
        assert!(
            !stdout.contains(url_encoded_lower),
            "lowercase url-encoded form must be redacted"
        );
        assert!(stdout.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn secret_args_wrong_type_rejected() {
        let f = Fixture::new();

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_exec",
            json!({
                "secret_name": f.secret_name,
                "command": "echo",
                "args": "not_an_array"
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("args"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_exec",
            json!({
                "secret_name": f.secret_name,
                "command": "echo",
                "args": [1, 2]
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("args"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_exec",
            json!({
                "secret_name": f.secret_name,
                "command": "echo",
                "ttl_seconds": "invalid"
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("ttl_seconds"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_exec",
            json!({
                "secret_name": 12345,
                "command": "echo"
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("secret_name"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_exec",
            json!({
                "secret_name": f.secret_name,
                "command": true
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("command"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_exec",
            json!({
                "secret_name": f.secret_name,
                "command": "echo",
                "env_var": 999
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("env_var"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_lend",
            json!({
                "secret_name": f.secret_name,
                "agent_id": "agent",
                "ttl_seconds": "invalid"
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("ttl_seconds"));
    }

    #[tokio::test]
    async fn secret_lend_ttl_zero_rejected() {
        let f = Fixture::new();

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_lend",
            json!({
                "secret_name": f.secret_name,
                "agent_id": "test_agent",
                "ttl_seconds": 0
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        let err = res["structuredContent"]["error"].as_str().unwrap();
        assert!(err.contains("ttl_seconds") && err.contains(">= 1"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_lend",
            json!({
                "secret_name": f.secret_name,
                "agent_id": "test_agent",
                "ttl_seconds": -5
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        let err = res["structuredContent"]["error"].as_str().unwrap();
        assert!(err.contains("ttl_seconds"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_exec",
            json!({
                "secret_name": f.secret_name,
                "command": "echo",
                "ttl_seconds": 0
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], true);
        let err = res["structuredContent"]["error"].as_str().unwrap();
        assert!(err.contains("ttl_seconds") && err.contains(">= 1"));

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_lend",
            json!({
                "secret_name": f.secret_name,
                "agent_id": "test_agent",
                "ttl_seconds": 999_999
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], false);
        assert_eq!(res["structuredContent"]["revoked"], false);
    }

    #[tokio::test]
    async fn secret_exec_rejects_invalid_env_var() {
        let f = Fixture::new();

        for disallowed in ["PATH", "LD_PRELOAD", "LD_LIBRARY_PATH", "HOME", "SHELL"] {
            let res = handle_secrets_tool(
                &f.ctx,
                "secret_exec",
                json!({
                    "secret_name": f.secret_name,
                    "command": "echo",
                    "env_var": disallowed
                }),
            )
            .await
            .expect("tool returns result");
            assert_eq!(
                res["isError"], true,
                "Disallowed {disallowed} must be rejected"
            );
            assert!(
                res["structuredContent"]["error"]
                    .as_str()
                    .unwrap()
                    .contains("env_var"),
                "Error must name env_var for {disallowed}"
            );
        }

        for invalid in [
            "A=B;x", "FOO=BAR", "1VAR", "FOO-BAR", "FOO BAR", "VAR;id", "",
        ] {
            let res = handle_secrets_tool(
                &f.ctx,
                "secret_exec",
                json!({
                    "secret_name": f.secret_name,
                    "command": "echo",
                    "env_var": invalid
                }),
            )
            .await
            .expect("tool returns result");
            assert_eq!(
                res["isError"], true,
                "Invalid env_var '{invalid}' must be rejected"
            );
            assert!(
                res["structuredContent"]["error"]
                    .as_str()
                    .unwrap()
                    .contains("env_var"),
                "Error must name env_var for '{invalid}'"
            );
        }

        let res = handle_secrets_tool(
            &f.ctx,
            "secret_exec",
            json!({
                "secret_name": f.secret_name,
                "command": "echo",
                "args": ["valid"],
                "env_var": "VALID_ENV_VAR_123"
            }),
        )
        .await
        .expect("tool returns result");
        assert_eq!(res["isError"], false);
    }
}
