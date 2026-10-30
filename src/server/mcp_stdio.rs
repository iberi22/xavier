//! MCP stdio transport server
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::server::mcp::auth::validate_stdio_connection;
use crate::server::mcp::session::dispatch_mcp_value;
use crate::{workspace::WorkspaceContext, AppState};
use anyhow::Result;
use std::io::{self, BufRead, Write};

/// Run stdio loop.
pub async fn run_stdio_loop(state: AppState, workspace: WorkspaceContext) -> Result<()> {
    validate_stdio_connection()?;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut input = String::new();

    loop {
        input.clear();
        if stdin.read_line(&mut input)? == 0 {
            break;
        }

        let payload = match serde_json::from_str(&input) {
            Ok(payload) => payload,
            Err(_) => continue,
        };

        let Some(response) = dispatch_stdio_value(state.clone(), workspace.clone(), payload)
            .await
            .ok()
            .flatten()
        else {
            continue;
        };

        let output = serde_json::to_vec(&response)?;
        stdout.write_all(&output)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }

    Ok(())
}

/// Subject of the local stdio identity; it carries no root credential.
pub(crate) const STDIO_LOCAL_SUBJECT: &str = "stdio_local";

pub(crate) async fn dispatch_stdio_value(
    state: AppState,
    workspace: WorkspaceContext,
    payload: serde_json::Value,
) -> Result<Option<serde_json::Value>, String> {
    let claims = crate::security::auth::Claims::new(
        STDIO_LOCAL_SUBJECT.to_string(),
        "local@xavier".to_string(),
        crate::security::auth::UserRole::Admin,
        chrono::Duration::hours(1),
    );
    // The local stdio identity carries no space token, so its ceiling is the
    // role clearance the HTTP transport would apply to the same claims.
    let caller = crate::server::mcp::tools_memory::McpCaller {
        space: None,
        clearance: crate::security::clearance::role_clearance(claims.role),
    };
    let write_key = crate::adapters::inbound::http::middleware::rate_limit::memory_write_caller_key(
        None,
        None,
        Some(claims.sub.as_str()),
        None,
    );
    crate::server::mcp::tools_memory::with_memory_write_caller_key(
        write_key,
        crate::server::mcp::tools_memory::with_mcp_caller(
            caller,
            dispatch_mcp_value(state, workspace, Some(&claims), payload),
        ),
    )
    .await
}
