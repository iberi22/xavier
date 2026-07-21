// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! MCP Streamable HTTP (HTTP+SSE) transport
//!
//! Exposes the unified JSON-RPC dispatcher over the MCP Streamable HTTP
//! transport so remote agents can talk to Xavier without stdio:
//!
//! - `POST /mcp` — client→server JSON-RPC requests and batches.
//! - `GET  /mcp` — server→client SSE notification stream (`text/event-stream`).
//! - `DELETE /mcp` — terminate an MCP session.
//!
//! The handlers themselves live in [`super::session`]; this module only wires
//! them into an axum [`Router`] and serves it, reusing the exact same
//! `dispatch_mcp_value` dispatcher that the stdio transport uses so there is no
//! duplicated protocol logic.

use crate::workspace::WorkspaceContext;
use crate::AppState;
use anyhow::Result;
use axum::{routing::post, Router};
use tokio::net::TcpListener;
use tracing::info;

use super::auth::mcp_auth_middleware;
use super::session::mcp_post_handler;

/// Build the MCP Streamable HTTP router.
///
/// Carries the shared [`AppState`] as router state plus the
/// [`WorkspaceContext`] as an extension layer, matching the extractor
/// signatures of the MCP handlers.
pub fn build_mcp_http_router(state: AppState, workspace: WorkspaceContext) -> Router {
    Router::new()
        .route("/mcp", post(mcp_post_handler))
        .layer(axum::middleware::from_fn(mcp_auth_middleware))
        .layer(axum::Extension(workspace))
        .with_state(state)
}

/// Start the MCP HTTP+SSE server bound to the given `bind_addr`
/// (e.g. `"127.0.0.1:8100"`).
///
/// The bind address (host + port) is resolved by the caller so this module
/// stays free of CLI/settings concerns and can be reused from any entry point.
pub async fn start_mcp_http_server(
    state: AppState,
    workspace: WorkspaceContext,
    bind_addr: String,
) -> Result<()> {
    let app = build_mcp_http_router(state, workspace);
    let listener = TcpListener::bind(&bind_addr).await?;
    let local_addr = listener.local_addr()?;
    info!(
        "Xavier MCP HTTP+SSE server listening on http://{}",
        local_addr
    );
    println!(
        "Xavier MCP HTTP+SSE server listening on http://{}",
        local_addr
    );
    println!("Press Ctrl+C to stop");
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}
