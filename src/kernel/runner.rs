//! Execution runner for proxy commands with token usage accounting

use super::filters;
use crate::observability::token_accounting::TRACKER;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Instant;

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

/// Execute a command in shell, apply RTK condensation filters, and log token savings to Xavier.
pub async fn execute_proxy_command(
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
        // Default strip ANSI and condense if excessively long
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

    // Standard approximation: 1 token ≈ 4 bytes
    let estimated_raw_tokens = (raw_bytes + 3) / 4;
    let estimated_filtered_tokens = (filtered_bytes + 3) / 4;
    let tokens_saved = estimated_raw_tokens.saturating_sub(estimated_filtered_tokens);
    let savings_percentage = if estimated_raw_tokens > 0 {
        (tokens_saved as f32 / estimated_raw_tokens as f32) * 100.0
    } else {
        0.0
    };

    // Track tokens into Xavier observability singleton
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
