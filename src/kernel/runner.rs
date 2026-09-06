//! Kernel command execution proxy and failure trace indexing.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};
use crate::observability::token_accounting::TRACKER;
use crate::utils::crypto::hex_encode;

/// Result of a proxied shell command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyCommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub failure_record: Option<MemoryRecord>,
}

/// Condenses command stdout and stderr into a concise failure/output snippet.
pub fn condense_output(stdout: &str, stderr: &str) -> String {
    let combined = if stderr.trim().is_empty() {
        stdout.to_string()
    } else if stdout.trim().is_empty() {
        stderr.to_string()
    } else {
        format!("STDOUT:\n{}\nSTDERR:\n{}", stdout.trim(), stderr.trim())
    };

    if combined.len() > 2000 {
        let prefix = &combined[..1000];
        let suffix = &combined[combined.len() - 1000..];
        format!(
            "{}\n\n[... condensed {} bytes ...]\n\n{}",
            prefix,
            combined.len() - 2000,
            suffix
        )
    } else {
        combined
    }
}

/// Indexes a failed command trace into Xavier memory.
///
/// Path format: `terminal/failures/{timestamp}_{hash}`
/// Kind: `failure_trace`
pub async fn index_command_failure(
    store: &dyn MemoryStore,
    workspace_id: &str,
    cmd_line: &str,
    exit_code: i32,
    failure_snippet: &str,
) -> Result<MemoryRecord> {
    let timestamp = Utc::now().timestamp();
    let mut hasher = Sha256::new();
    hasher.update(cmd_line.as_bytes());
    hasher.update(failure_snippet.as_bytes());
    let hash = hex_encode(&hasher.finalize())[..8].to_string();

    let path = format!("terminal/failures/{}_{}", timestamp, hash);
    let id = stable_key("failure_trace", &[workspace_id, &path]);

    let metadata = json!({
        "command": cmd_line,
        "exit_code": exit_code,
        "kind": "failure_trace",
    });

    let record = MemoryRecord {
        id,
        workspace_id: workspace_id.to_string(),
        path,
        content: failure_snippet.to_string(),
        metadata,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        primary: true,
        ..Default::default()
    };

    store.put(record.clone()).await?;
    Ok(record)
}

/// Executes a shell command via proxy, records token usage in TRACKER, applies output condensation,
/// and if exit_code != 0 and workspace_id/store are available, automatically indexes the failure trace.
pub async fn execute_proxy_command(
    cmd_line: &str,
    workspace_id: Option<&str>,
    store: Option<&dyn MemoryStore>,
) -> Result<ProxyCommandResult> {
    #[cfg(target_os = "windows")]
    let output = tokio::process::Command::new("cmd")
        .args(["/C", cmd_line])
        .output()
        .await?;

    #[cfg(not(target_os = "windows"))]
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd_line)
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    let original_tokens = (stdout.len() + stderr.len()) / 4;
    let failure_snippet = condense_output(&stdout, &stderr);
    let optimized_tokens = failure_snippet.len() / 4;

    let session_id = workspace_id.unwrap_or("kernel-proxy").to_string();
    TRACKER
        .track(session_id, original_tokens, optimized_tokens, 0.002)
        .await;

    let failure_record = if exit_code != 0 {
        if let (Some(ws_id), Some(st)) = (workspace_id, store) {
            index_command_failure(st, ws_id, cmd_line, exit_code, &failure_snippet)
                .await
                .ok()
        } else {
            None
        }
    } else {
        None
    };

    Ok(ProxyCommandResult {
        stdout,
        stderr,
        exit_code,
        failure_record,
    })
}
