//! In-process secret execution engine.
//!
//! Spawns child processes with ephemeral secrets injected exclusively via
//! environment variables, with leases managed and audited through
//! [`KeyLendingEngine`]. No secret is ever passed via command line arguments,
//! logged, or exposed in outcome structures.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{compiler_fence, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::coordination::secrets::{KeyLendingEngine, SecretLease};
use crate::secrets::vault::HardwareVault;

/// Error type for secret-injected child process execution.
///
/// No variant embeds a secret value: at most the secret *name*, a child I/O
/// message and the TTL are ever reported.
#[derive(Debug, Error)]
pub enum ExecSecretError {
    #[error("Vault error: {0}")]
    Vault(String),
    #[error("Lease error: {0}")]
    Lease(String),
    #[error("Child process I/O error: {0}")]
    Io(String),
    #[error("Execution timed out after {0}s")]
    Timeout(u64),
}

pub type ExecSecretResult<T> = Result<T, ExecSecretError>;

/// Specification for executing a command with an injected secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretExecSpec {
    pub secret_name: String,
    pub env_var: String,
    pub command: String,
    pub args: Vec<String>,
    pub ttl_secs: u64,
    pub agent_id: String,
    #[serde(skip)]
    pub stdout_path: Option<PathBuf>,
}

impl SecretExecSpec {
    /// New.
    pub fn new(
        secret_name: impl Into<String>,
        env_var: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
        ttl_secs: u64,
        agent_id: impl Into<String>,
    ) -> Self {
        Self {
            secret_name: secret_name.into(),
            env_var: env_var.into(),
            command: command.into(),
            args,
            ttl_secs,
            agent_id: agent_id.into(),
            stdout_path: None,
        }
    }

    /// With stdout path.
    pub fn with_stdout_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.stdout_path = Some(path.into());
        self
    }
}

/// Execution outcome that structurally cannot represent or leak the secret value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretExecOutcome {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub killed: bool,
    pub duration_ms: u64,
}

/// Zeroizing wrapper around a memory buffer holding the secret value.
struct ZeroizingBuffer(Vec<u8>);

impl ZeroizingBuffer {
    fn new(s: String) -> Self {
        Self(s.into_bytes())
    }
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

#[allow(unsafe_code)]
impl Drop for ZeroizingBuffer {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            unsafe {
                std::ptr::write_volatile(b, 0);
            }
        }
        compiler_fence(Ordering::SeqCst);
        self.0.clear();
    }
}

/// Audit reason recorded when the guard is dropped without an explicit revoke,
/// which only happens while a panic unwinds the stack.
const UNWOUND_REASON: &str = "exec aborted by panic unwind";

/// Result of the child-execution phase plus the reason that must be audited
/// when the lease is revoked, so no exit path can forget the revocation.
type ChildPhase = (Result<SecretExecOutcome, ExecSecretError>, &'static str);

/// RAII guard guaranteeing the lease is revoked exactly once.
///
/// The explicit [`LeaseGuard::revoke`] runs on every normal exit path
/// (success, error and timeout). It consumes the guard, so [`Drop`] becomes a
/// no-op afterwards. [`Drop`] only has work to do when the future was dropped
/// mid-flight by a panic unwind, in which case the revocation is handed to the
/// ambient Tokio runtime as a detached task.
struct LeaseGuard {
    engine: Arc<KeyLendingEngine>,
    token: Option<String>,
}

impl LeaseGuard {
    fn new(engine: Arc<KeyLendingEngine>, token: String) -> Self {
        Self {
            engine,
            token: Some(token),
        }
    }

    /// Revoke the lease and audit `reason`. Consuming `self` guarantees
    /// [`Drop`] cannot revoke a second time.
    async fn revoke(mut self, reason: &str) {
        if let Some(token) = self.token.take() {
            let _ = self.engine.revoke(&token, reason).await;
        }
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            return;
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let engine = Arc::clone(&self.engine);
                handle.spawn(async move {
                    let _ = engine.revoke(&token, UNWOUND_REASON).await;
                });
            }
            Err(_) => {
                tracing::error!(
                    "lease revoked only on a best-effort basis: no async runtime available to \
                     unwind the revocation"
                );
            }
        }
    }
}

/// Resolve `spec.secret_name` in `vault`, lend the value under the TTL
/// watchdog, run the child with the value injected into exactly one
/// environment variable, and revoke the lease on every exit path.
///
/// The vault is a parameter so the caller owns the storage seam: production
/// passes `HardwareVault::new("xavier")`, tests pass an isolated vault. When the
/// value cannot be resolved the call fails closed, with no `std::env` and no
/// `.env` fallback, and the child is never spawned.
pub async fn run_with_secret(
    spec: &SecretExecSpec,
    engine: Arc<KeyLendingEngine>,
    vault: &HardwareVault,
) -> Result<SecretExecOutcome, ExecSecretError> {
    let lease = {
        let value = vault
            .get_secret(&spec.secret_name)
            .map_err(|e| ExecSecretError::Vault(e.to_string()))?;
        let secret_buf = ZeroizingBuffer::new(value);
        engine
            .lend(
                &spec.secret_name,
                Some(secret_buf.as_str()),
                &spec.agent_id,
                spec.ttl_secs,
            )
            .await
            .map_err(|e| ExecSecretError::Lease(e.to_string()))?
    };

    // From here on the lease exists, so the guard owns its lifetime: success,
    // error, timeout and panic unwind all revoke it exactly once.
    let guard = LeaseGuard::new(Arc::clone(&engine), lease.token.clone());
    let (result, reason) = run_child_with_lease(spec, lease).await;
    guard.revoke(reason).await;
    result
}

/// Spawn, run and reap the child process. Never revokes: revocation is the
/// caller's single exit path so that no early `return` can leak the lease.
async fn run_child_with_lease(spec: &SecretExecSpec, lease: SecretLease) -> ChildPhase {
    let Some(secret_val_raw) = lease.secret_value else {
        return (
            Err(ExecSecretError::Vault(
                "Vault returned no secret value".to_string(),
            )),
            "lease carried no secret value",
        );
    };
    let secret_buf = ZeroizingBuffer::new(secret_val_raw);

    let mut cmd = tokio::process::Command::new(&spec.command);
    cmd.kill_on_drop(true);
    cmd.args(&spec.args);
    cmd.env(&spec.env_var, secret_buf.as_str());
    cmd.stderr(Stdio::null());

    if let Some(ref path) = spec.stdout_path {
        let file = match std::fs::File::create(path) {
            Ok(file) => file,
            Err(e) => {
                return (
                    Err(ExecSecretError::Io(format!(
                        "Create stdout file failed: {e}"
                    ))),
                    "stdout file could not be created",
                );
            }
        };
        cmd.stdout(Stdio::from(file));
    } else {
        cmd.stdout(Stdio::null());
    }

    let start = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return (
                Err(ExecSecretError::Io(format!("Spawn failed: {e}"))),
                "child spawn failed",
            );
        }
    };
    drop(secret_buf);

    let ttl = Duration::from_secs(spec.ttl_secs);
    let (status, timed_out, killed) = match tokio::time::timeout(ttl, child.wait()).await {
        Ok(Ok(exit_status)) => (Some(exit_status), false, false),
        Ok(Err(e)) => {
            let _ = child.kill().await;
            return (
                Err(ExecSecretError::Io(format!("Wait failed: {e}"))),
                "wait error",
            );
        }
        Err(_) => {
            let _ = child.kill().await;
            let exit_status = child.wait().await.ok();
            (exit_status, true, true)
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = status.and_then(|s| s.code());
    let success = !timed_out && !killed && status.map(|s| s.success()).unwrap_or(false);
    let reason = if timed_out {
        "TTL watchdog expired"
    } else if success {
        "child exited"
    } else {
        "child exited with non-zero status"
    };

    (
        Ok(SecretExecOutcome {
            exit_code,
            success,
            timed_out,
            killed,
            duration_ms,
        }),
        reason,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codebase::connection_manager::ConnectionManager;
    use crate::secrets::audit::QmdAuditLogger;
    use rusqlite::params;

    /// Environment variable the child is given. Unique to this module so the
    /// parent process environment can be asserted untouched.
    const CANARY_VAR: &str = "XAVIER_EXEC_CANARY";
    /// Synthetic value. No real credential is ever used by these tests.
    const CANARY: &str = "canary-4f2b91c7-exec-only";

    /// Isolated vault + metrics DB per fixture so parallel tests never share leases.
    struct Fixture {
        key: String,
        engine: Arc<KeyLendingEngine>,
        vault: HardwareVault,
        dir: tempfile::TempDir,
        project_id: String,
    }

    impl Fixture {
        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        async fn run(&self, spec: &SecretExecSpec) -> Result<SecretExecOutcome, ExecSecretError> {
            let engine = Arc::clone(&self.engine);
            run_with_secret(spec, engine, &self.vault).await
        }
    }

    async fn setup(value: Option<&str>) -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let store_dir = dir.path().join("secrets");
        let vault = HardwareVault::new("x").isolated(store_dir, [7u8; 32]);
        let key = format!("K_{}", uuid::Uuid::new_v4());
        if let Some(value) = value {
            vault.store_secret(&key, value).expect("store canary");
        }
        let project_id = format!("test_exec_secrets_{}", uuid::Uuid::new_v4());
        ConnectionManager::global()
            .connect_with_path(&project_id, dir.path().join("metrics.db"))
            .expect("connect isolated metrics db");
        let audit = Box::new(QmdAuditLogger::for_project(&project_id));
        audit.init_schema_async().await.expect("audit schema");
        let engine =
            Arc::new(KeyLendingEngine::new(audit, None).with_leases_project(project_id.clone()));
        Fixture {
            key,
            engine,
            vault,
            dir,
            project_id,
        }
    }

    /// The `agent_id` is the unique secret key, so audit rows written by
    /// concurrently running tests can never be mistaken for this fixture's.
    fn spec(key: &str, command: &str, args: &[&str], ttl_secs: u64) -> SecretExecSpec {
        let args = args.iter().map(|arg| (*arg).to_string()).collect();
        SecretExecSpec::new(key, CANARY_VAR, command, args, ttl_secs, key)
    }

    /// `(LEND, REVOKE)` audit row counts for one agent, read through the
    /// fixture's own isolated metrics connection.
    async fn audit_counts(project_id: &str, agent_id: &str) -> (i64, i64) {
        let lends = audit_rows(project_id, agent_id, "LEND").await;
        let revokes = audit_rows(project_id, agent_id, "REVOKE").await;
        (lends, revokes)
    }

    /// Number of audit rows for one agent and one event type.
    async fn audit_rows(project_id: &str, agent_id: &str, event: &str) -> i64 {
        let project_id = project_id.to_string();
        let agent = agent_id.to_string();
        let kind = event.to_string();
        ConnectionManager::global()
            .with_conn(&project_id, move |conn| {
                let count = conn.query_row(
                    "SELECT COUNT(*) FROM secret_audit_logs
                     WHERE agent_id = ?1 AND event_type = ?2",
                    params![agent, kind],
                    |row| row.get(0),
                )?;
                Ok(count)
            })
            .await
            .expect("metrics audit query")
    }

    /// The audit logger writes through spawned tasks, so poll briefly for the
    /// LEND/REVOKE pair and return what was actually observed.
    async fn wait_for_audit_pair(project_id: &str, agent_id: &str) -> (i64, i64) {
        for _ in 0..100 {
            let counts = audit_counts(project_id, agent_id).await;
            if counts == (1, 1) {
                return counts;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        audit_counts(project_id, agent_id).await
    }

    #[tokio::test]
    async fn test_exec_injects_value_into_child_env_only() {
        let f = setup(Some(CANARY)).await;
        let out = f.path("child-stdout.txt");
        let spec = spec(&f.key, "printenv", &[CANARY_VAR], 30).with_stdout_path(&out);

        let outcome = f.run(&spec).await.expect("exec");

        assert!(outcome.success);
        let printed = std::fs::read_to_string(&out).expect("child stdout");
        assert_eq!(printed.trim_end(), CANARY);
        assert!(!format!("{:?}", outcome).contains(CANARY));
        assert!(!serde_json::to_string(&outcome)
            .expect("json")
            .contains(CANARY));
        assert!(f.engine.list_leases().await.is_empty());
    }

    #[tokio::test]
    async fn test_exec_outcome_never_contains_value() {
        let f = setup(Some(CANARY)).await;
        let out = f.path("echoed-canary.txt");
        let spec = spec(&f.key, "printenv", &[CANARY_VAR], 30).with_stdout_path(&out);

        let outcome = f.run(&spec).await.expect("exec");

        let printed = std::fs::read_to_string(&out).expect("child stdout");
        assert!(printed.contains(CANARY));
        let debug = format!("{outcome:?}");
        let pretty = format!("{outcome:#?}");
        let json = serde_json::to_string(&outcome).expect("json");
        for rendered in [debug, pretty, json] {
            assert!(!rendered.contains(CANARY));
        }
        assert!(outcome.success);
    }

    #[tokio::test]
    async fn test_exec_revokes_lease_on_child_exit() {
        let f = setup(Some(CANARY)).await;
        let spec = spec(&f.key, "true", &[], 30);

        let outcome = f.run(&spec).await.expect("exec");

        assert!(outcome.success);
        assert_eq!(outcome.exit_code, Some(0));
        assert!(!outcome.timed_out);
        assert!(!outcome.killed);
        assert!(f.engine.list_leases().await.is_empty());
        assert_eq!(wait_for_audit_pair(&f.project_id, &f.key).await, (1, 1));
    }

    #[tokio::test]
    async fn test_exec_revokes_lease_on_nonzero_exit() {
        let f = setup(Some(CANARY)).await;
        let spec = spec(&f.key, "false", &[], 30);

        let outcome = f.run(&spec).await.expect("exec");

        assert!(!outcome.success);
        assert_eq!(outcome.exit_code, Some(1));
        assert!(!outcome.timed_out);
        assert!(!outcome.killed);
        assert!(f.engine.list_leases().await.is_empty());
        assert_eq!(wait_for_audit_pair(&f.project_id, &f.key).await, (1, 1));
    }

    #[tokio::test]
    async fn test_exec_kills_child_on_ttl_expiry() {
        let f = setup(Some(CANARY)).await;
        let spec = spec(&f.key, "sleep", &["30"], 1);

        let start = Instant::now();
        let outcome = f.run(&spec).await.expect("exec");
        let elapsed = start.elapsed();

        assert!(outcome.timed_out);
        assert!(outcome.killed);
        assert!(!outcome.success);
        assert!(elapsed < Duration::from_secs(5));
        assert!(f.engine.list_leases().await.is_empty());
        assert_eq!(wait_for_audit_pair(&f.project_id, &f.key).await, (1, 1));
    }

    #[tokio::test]
    async fn test_exec_fails_closed_when_vault_unavailable() {
        let f = setup(None).await;
        let marker = f.path("side-effect-marker");
        let marker_arg = marker.to_str().expect("utf-8 path");
        let spec = spec(&f.key, "touch", &[marker_arg], 30);

        let result = f.run(&spec).await;

        assert!(matches!(result, Err(ExecSecretError::Vault(_))));
        assert!(!marker.exists());
        assert!(f.engine.list_leases().await.is_empty());
        assert_eq!(audit_counts(&f.project_id, &f.key).await, (0, 0));
    }

    #[tokio::test]
    async fn test_exec_revokes_lease_when_stdout_path_is_unusable() {
        let f = setup(Some(CANARY)).await;
        let bad_out = f.path("missing-dir/out.txt");
        let marker = f.path("side-effect-marker");
        let marker_arg = marker.to_str().expect("utf-8 path");
        let spec = spec(&f.key, "touch", &[marker_arg], 30).with_stdout_path(&bad_out);

        let result = f.run(&spec).await;

        assert!(matches!(result, Err(ExecSecretError::Io(_))));
        assert!(!marker.exists());
        assert!(f.engine.list_leases().await.is_empty());
        assert_eq!(wait_for_audit_pair(&f.project_id, &f.key).await, (1, 1));
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(audit_counts(&f.project_id, &f.key).await, (1, 1));
    }
}
