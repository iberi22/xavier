//! Kernel command execution proxy, RTK token reduction, and failure trace indexing.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::process::Command;
use std::time::Instant;

use super::filters;
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};
use crate::observability::token_accounting::TRACKER;
use crate::utils::crypto::hex_encode;

/// Detailed result of a proxied command with token accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub command: String,
    pub exit_code: i32,
    pub raw_bytes: usize,
    pub filtered_bytes: usize,
    pub estimated_raw_tokens: usize,
    pub estimated_filtered_tokens: usize,
    pub tokens_saved: usize,
    pub savings_percentage: f32,
    pub duration_ms: u128,
    pub output: String,
}

/// Result of a proxied shell command execution (compat with PR #1994).
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

/// Execute a command in shell, apply RTK condensation filters, and log token savings to Xavier.
pub async fn execute_rtk_command(
    cmd_line: &str,
    cwd: Option<&str>,
    session_id: Option<&str>,
) -> Result<ExecutionResult> {
    let start = Instant::now();

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd_line]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd_line]);
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute command: {}", cmd_line))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout_raw = String::from_utf8_lossy(&output.stdout);
    let stderr_raw = String::from_utf8_lossy(&output.stderr);

    let combined_raw = if !stderr_raw.is_empty() {
        format!("{}\n{}", stdout_raw, stderr_raw)
    } else {
        stdout_raw.to_string()
    };

    let raw_bytes = combined_raw.len();

    // Determine filter strategy based on command prefix
    let trimmed = cmd_line.trim();
    let filtered_output = if trimmed.starts_with("cargo ") {
        filters::filter_cargo(&combined_raw)
    } else if trimmed.starts_with("git ") {
        filters::filter_git(&combined_raw)
    } else if trimmed.starts_with("grep ") || trimmed.starts_with("rg ") {
        filters::filter_grep(&combined_raw)
    } else {
        let cleaned = filters::strip_ansi(&combined_raw);
        let lines: Vec<&str> = cleaned.lines().collect();
        if lines.len() > 100 {
            let mut head = lines.iter().take(80).copied().collect::<Vec<_>>().join("\n");
            head.push_str(&format!("\n... [{} lines truncated]", lines.len() - 80));
            head
        } else {
            cleaned
        }
    };

    let filtered_bytes = filtered_output.len();

    let estimated_raw_tokens = (raw_bytes + 3) / 4;
    let estimated_filtered_tokens = (filtered_bytes + 3) / 4;
    let tokens_saved = estimated_raw_tokens.saturating_sub(estimated_filtered_tokens);
    let savings_percentage = if estimated_raw_tokens > 0 {
        (tokens_saved as f32 / estimated_raw_tokens as f32) * 100.0
    } else {
        0.0
    };

    let active_session = session_id.unwrap_or("xavier_kernel_cli").to_string();
    TRACKER
        .track(
            active_session,
            estimated_raw_tokens,
            estimated_filtered_tokens,
            0.01,
        )
        .await;

    Ok(ExecutionResult {
        command: cmd_line.to_string(),
        exit_code,
        raw_bytes,
        filtered_bytes,
        estimated_raw_tokens,
        estimated_filtered_tokens,
        tokens_saved,
        savings_percentage,
        duration_ms: start.elapsed().as_millis(),
        output: filtered_output,
    })
}
