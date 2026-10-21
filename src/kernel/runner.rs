//! Kernel command execution proxy, RTK token reduction, and failure trace indexing.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::filters;
use crate::domain::cycle_breaks::w30_12::{stable_key, MemoryConsumerStore, MemoryRecord};
use crate::observability::token_accounting::TRACKER;
use crate::utils::crypto::hex_encode;

/// Default command execution timeout (30 seconds).
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Default output cap per stream (stdout and stderr) in bytes (64 KiB).
pub const DEFAULT_OUTPUT_CAP_BYTES: usize = 64 * 1024;

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
    #[serde(default)]
    pub truncated: bool,
}

/// Result of a proxied shell command execution (compat with PR #1994).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyCommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub failure_record: Option<MemoryRecord>,
    #[serde(default)]
    pub truncated: bool,
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
    store: &dyn MemoryConsumerStore,
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

    store.put_record(record.clone()).await?;
    Ok(record)
}

fn resolve_timeout(timeout: Option<Duration>) -> Duration {
    if let Some(t) = timeout {
        return t;
    }
    if let Ok(val) = std::env::var("XAVIER_COMMAND_TIMEOUT_SECS") {
        if let Ok(secs) = val.parse::<u64>() {
            return Duration::from_secs(secs);
        }
    }
    DEFAULT_COMMAND_TIMEOUT
}

/// Reads from an async reader up to `cap` bytes into a buffer.
/// If more data is available, it continues draining so the child process does not block on a full pipe,
/// while keeping the memory buffer capped at `cap` bytes and marking `truncated = true`.
async fn read_stream_capped<R: AsyncReadExt + Unpin>(
    mut reader: R,
    cap: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut buf = Vec::with_capacity(std::cmp::min(cap, 8192));
    let mut chunk = [0u8; 8192];
    let mut truncated = false;

    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        if buf.len() < cap {
            let available = cap - buf.len();
            let to_copy = std::cmp::min(n, available);
            buf.extend_from_slice(&chunk[..to_copy]);
            if n > to_copy {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }

    Ok((buf, truncated))
}

struct ProcessOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    truncated: bool,
    exit_code: i32,
}

async fn run_process_capped(
    cmd_line: &str,
    cwd: Option<&str>,
    timeout_duration: Option<Duration>,
    output_cap_bytes: usize,
) -> Result<ProcessOutput> {
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

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn command: {}", cmd_line))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let read_and_wait = async {
        let read_out = async {
            if let Some(pipe) = stdout_pipe {
                read_stream_capped(pipe, output_cap_bytes).await
            } else {
                Ok((Vec::new(), false))
            }
        };

        let read_err = async {
            if let Some(pipe) = stderr_pipe {
                read_stream_capped(pipe, output_cap_bytes).await
            } else {
                Ok((Vec::new(), false))
            }
        };

        let (stdout_res, stderr_res, status_res) = tokio::join!(read_out, read_err, child.wait());

        let (stdout_buf, out_truncated) = stdout_res
            .with_context(|| format!("Failed reading stdout for command: {}", cmd_line))?;
        let (stderr_buf, err_truncated) = stderr_res
            .with_context(|| format!("Failed reading stderr for command: {}", cmd_line))?;
        let status = status_res
            .with_context(|| format!("Failed waiting for child process: {}", cmd_line))?;

        let exit_code = status.code().unwrap_or(-1);
        let truncated = out_truncated || err_truncated;

        Ok::<_, anyhow::Error>((stdout_buf, stderr_buf, truncated, exit_code))
    };

    let Some(timeout_duration) = timeout_duration else {
        let (stdout, stderr, truncated, exit_code) = read_and_wait.await?;
        return Ok(ProcessOutput {
            stdout,
            stderr,
            truncated,
            exit_code,
        });
    };
    match tokio::time::timeout(timeout_duration, read_and_wait).await {
        Ok(res) => {
            let (stdout, stderr, truncated, exit_code) = res?;
            Ok(ProcessOutput {
                stdout,
                stderr,
                truncated,
                exit_code,
            })
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            anyhow::bail!(
                "Command timed out after {}ms: {}",
                timeout_duration.as_millis(),
                cmd_line
            );
        }
    }
}

/// Executes a shell command via proxy, records token usage in TRACKER, applies output condensation,
/// and if exit_code != 0 and workspace_id/store are available, automatically indexes the failure trace.
pub async fn execute_proxy_command(
    cmd_line: &str,
    workspace_id: Option<&str>,
    store: Option<&dyn MemoryConsumerStore>,
) -> Result<ProxyCommandResult> {
    let proc_out = run_process_capped(
        cmd_line,
        None,
        Some(DEFAULT_COMMAND_TIMEOUT),
        DEFAULT_OUTPUT_CAP_BYTES,
    )
    .await?;

    let stdout = String::from_utf8_lossy(&proc_out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&proc_out.stderr).to_string();
    let exit_code = proc_out.exit_code;

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
        truncated: proc_out.truncated,
    })
}

/// Execute a command in shell, apply RTK condensation filters, and log token savings to Xavier.
pub async fn execute_rtk_command(
    cmd_line: &str,
    cwd: Option<&str>,
    session_id: Option<&str>,
) -> Result<ExecutionResult> {
    // CLI `xavier exec` wraps long-running commands (builds, test suites): no timeout, no cap.
    execute_rtk_command_with_options(cmd_line, cwd, session_id, None, Some(usize::MAX)).await
}

/// Bounded execution for remote callers (MCP): default timeout
/// (`XAVIER_COMMAND_TIMEOUT_SECS` or 30 s) and a 64 KiB cap per stream.
pub async fn execute_rtk_command_bounded(
    cmd_line: &str,
    cwd: Option<&str>,
    session_id: Option<&str>,
) -> Result<ExecutionResult> {
    execute_rtk_command_with_options(
        cmd_line,
        cwd,
        session_id,
        Some(resolve_timeout(None)),
        Some(DEFAULT_OUTPUT_CAP_BYTES),
    )
    .await
}

/// Execute a command in shell with a specified timeout.
pub async fn execute_rtk_command_with_timeout(
    cmd_line: &str,
    cwd: Option<&str>,
    session_id: Option<&str>,
    timeout: Duration,
) -> Result<ExecutionResult> {
    execute_rtk_command_with_options(cmd_line, cwd, session_id, Some(timeout), None).await
}

/// Execute a command in shell with configurable timeout and output capping.
pub async fn execute_rtk_command_with_options(
    cmd_line: &str,
    cwd: Option<&str>,
    session_id: Option<&str>,
    timeout_opt: Option<Duration>,
    cap_opt: Option<usize>,
) -> Result<ExecutionResult> {
    let start = Instant::now();
    let cap = cap_opt.unwrap_or(DEFAULT_OUTPUT_CAP_BYTES);

    let proc_out = run_process_capped(cmd_line, cwd, timeout_opt, cap).await?;

    let exit_code = proc_out.exit_code;
    let stdout_raw = String::from_utf8_lossy(&proc_out.stdout);
    let stderr_raw = String::from_utf8_lossy(&proc_out.stderr);

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
            let mut head = lines
                .iter()
                .take(80)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            head.push_str(&format!("\n... [{} lines truncated]", lines.len() - 80));
            head
        } else {
            cleaned
        }
    };

    let filtered_bytes = filtered_output.len();

    let estimated_raw_tokens = raw_bytes.div_ceil(4);
    let estimated_filtered_tokens = filtered_bytes.div_ceil(4);
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
        truncated: proc_out.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_command_times_out() {
        let start = Instant::now();
        let timeout = Duration::from_millis(200);

        #[cfg(not(target_os = "windows"))]
        let cmd = "sleep 5";
        #[cfg(target_os = "windows")]
        let cmd = "powershell -Command Start-Sleep -Seconds 5";

        let result = execute_rtk_command_with_timeout(cmd, None, None, timeout).await;
        let elapsed = start.elapsed();

        assert!(
            result.is_err(),
            "Expected command to time out, got: {:?}",
            result
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("timed out") || err_msg.contains("timeout"),
            "Expected timeout error message, got: {}",
            err_msg
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "Command should have timed out within ~200ms, took: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn run_command_caps_output() {
        #[cfg(not(target_os = "windows"))]
        let cmd = "yes | head -c 200000";
        #[cfg(target_os = "windows")]
        let cmd = "powershell -Command \"[string]::new('y', 200000)\"";

        let result = execute_rtk_command_bounded(cmd, None, None).await;
        assert!(
            result.is_ok(),
            "Expected command to succeed, got: {:?}",
            result.err()
        );

        let res = result.unwrap();
        assert_eq!(res.exit_code, 0, "Command should have exited cleanly");
        assert!(res.truncated, "Result should be marked as truncated");
        assert!(
            res.raw_bytes <= DEFAULT_OUTPUT_CAP_BYTES,
            "raw_bytes ({}) must not exceed cap ({})",
            res.raw_bytes,
            DEFAULT_OUTPUT_CAP_BYTES
        );
        assert_eq!(
            res.raw_bytes, DEFAULT_OUTPUT_CAP_BYTES,
            "raw_bytes should match the cap of 64 KiB"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn cli_exec_is_not_capped() {
        let res = execute_rtk_command("yes | head -c 200000", None, None)
            .await
            .unwrap();
        assert!(!res.truncated);
        assert_eq!(res.raw_bytes, 200000);
    }

    #[tokio::test]
    async fn run_command_normal_execution() {
        #[cfg(not(target_os = "windows"))]
        let cmd = "echo 'hello world'";
        #[cfg(target_os = "windows")]
        let cmd = "cmd /c echo hello world";

        let result = execute_rtk_command(cmd, None, None).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.truncated);
        assert!(result.output.contains("hello world"));
    }
}
