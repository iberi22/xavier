//! Maloca ops domain — Xavier-hosted `/maloca/*` API.
//!
//! Primary dogfood host for Maloca (panel `MalocaView`). Backoffice consumes
//! the same surface via `@swal/maloca-client`. PWA comes later.
//!
//! See `docs/SWAL/MALOCA_SUPPORT_WORKSPACE.md` and ADR-002.

pub mod beliefs;
pub mod commits;
pub mod core_bridge;
pub mod data_node;
mod handlers;
mod params;
mod store;
pub mod timeline;
pub mod types;
pub mod universal;
pub mod ws;

pub use store::MalocaStore;
pub use types::*;

use axum::routing::{delete, get, post};
use axum::Router;
use std::sync::Arc;

/// Build the `/maloca` router (mount with `.nest("/maloca", …)` + `Extension(store)`).
pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
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
        .route("/feed/status", get(handlers::feed_status))
        .route(
            "/consent",
            get(handlers::list_consents).post(handlers::register_consent),
        )
        .route(
            "/consent/{node_id}",
            get(handlers::get_consent).delete(handlers::revoke_consent),
        )
}

/// Convenience: nested `/maloca` tree with store + consent registry extensions.
pub fn nested_router<S: Clone + Send + Sync + 'static>(store: Arc<MalocaStore>) -> Router<S> {
    nested_router_with_consent(store, data_node::ConsentRegistry::new_std())
}

/// Convenience: nested `/maloca` tree with both store and consent registry.
pub fn nested_router_with_consent<S: Clone + Send + Sync + 'static>(
    store: Arc<MalocaStore>,
    consent: Arc<data_node::ConsentRegistry>,
) -> Router<S> {
    Router::new()
        .nest("/maloca", router())
        .layer(axum::Extension(store))
        .layer(axum::Extension(consent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_initialization() {
        let _r: axum::Router<()> = router();
    }

    #[test]
    fn test_nested_router_initialization() {
        let dir = std::env::temp_dir().join(format!("maloca-mod-test-{}", uuid::Uuid::new_v4()));
        let store = MalocaStore::open(&dir);
        let _r: axum::Router<()> = nested_router(store);
        let _ = std::fs::remove_dir_all(dir);
    }
}
