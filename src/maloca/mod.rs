//! Maloca ops domain — Xavier-hosted `/maloca/*` API.
//!
//! Primary dogfood host for Maloca (panel `MalocaView`). Backoffice consumes
//! the same surface via `@swal/maloca-client`. PWA comes later.
//!
//! See `docs/SWAL/MALOCA_SUPPORT_WORKSPACE.md` and ADR-002.

mod handlers;
mod params;
mod store;
pub mod types;

pub use store::MalocaStore;
pub use types::*;

use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;

/// Build the `/maloca` router (mount with `.nest("/maloca", …)` + `Extension(store)`).
pub fn router() -> Router {
    Router::new()
        .route("/pack", get(handlers::pack))
        .route("/backlog", get(handlers::backlog))
        .route(
            "/support",
            get(handlers::list_support).post(handlers::create_support),
        )
        .route("/reviews", get(handlers::list_reviews))
        .route("/inbox", get(handlers::list_inbox))
        .route("/inbox/{id}/claim", post(handlers::claim))
        .route("/inbox/{id}/complete", post(handlers::complete))
        .route("/rewards", get(handlers::rewards))
        .route("/mesh", get(handlers::mesh))
        .route("/nodes", get(handlers::list_nodes))
        .route("/params", get(handlers::params))
        .route(
            "/proposals",
            get(handlers::list_proposals).post(handlers::create_proposal),
        )
        .route("/proposals/{id}/vote", post(handlers::cast_vote))
        .route("/votes", get(handlers::list_votes))
        .route("/decisions", get(handlers::list_decisions))
        .route(
            "/manager-actions",
            get(handlers::list_manager_actions).post(handlers::manager_action),
        )
}

/// Convenience: nested `/maloca` tree with store extension applied.
pub fn nested_router(store: Arc<MalocaStore>) -> Router {
    Router::new()
        .nest("/maloca", router())
        .layer(axum::Extension(store))
}
