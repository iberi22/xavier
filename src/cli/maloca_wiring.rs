use axum::{routing::get, Router};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::cli::http_setup::maloca_mutation_auth_middleware;
use crate::cli::state::CliState;

pub struct MalocaWiring {
    pub router: axum::Router<()>,
    pub harvester_handle: Option<tokio::task::JoinHandle<()>>,
}

pub fn maloca_static_routes() -> Router<CliState> {
    Router::new()
        // ── Maloca Timeline Export API (MS-002) ──────────────────────────
        .route(
            "/maloca/timeline",
            get(xavier::maloca::timeline::timeline_export),
        )
        .route(
            "/maloca/timeline/sessions",
            get(xavier::maloca::timeline::timeline_sessions),
        )
        .route(
            "/maloca/timeline/{id}/context",
            get(xavier::maloca::timeline::timeline_event_context),
        )
        // ── Maloca Commit Chronicle API (MS-003) ─────────────────────────
        .route(
            "/maloca/commits/graph",
            get(xavier::maloca::commits::commits_graph),
        )
        // ── Maloca WebSocket Live Feed (MS-004) ──────────────────────────
        .route("/maloca/ws/feed", get(xavier::maloca::ws::ws_live_feed))
        // NOTE: GET /maloca/feed/status se registra en maloca::nested_router (src/maloca/mod.rs:53)
        // y se mergea abajo — NO duplicar aquí (panic axum "Overlapping method route").
        // ── Maloca Belief Graph Confidence (MS-005) ──────────────────────
        .route(
            "/maloca/beliefs",
            get(xavier::maloca::beliefs::beliefs_snapshot),
        )
        .route(
            "/maloca/beliefs/path",
            get(xavier::maloca::beliefs::belief_path),
        )
}

pub fn build_maloca_router(state: &CliState, shutdown: CancellationToken) -> MalocaWiring {
    // Maloca ops API — public local dogfood (matches @swal/maloca-client; no token).
    let maloca_store = state.maloca.clone();

    // Maloca ops API — public local dogfood for reads (matches @swal/maloca-client;
    // no token needed on GET/HEAD), but every mutating verb (POST/PUT/PATCH/DELETE)
    // requires the same token as the rest of the API via
    // `maloca_mutation_auth_middleware`. This router is merged in BEFORE the
    // `CorsLayer`/timeout layer below so those layers wrap Maloca too — previously
    // they were applied first and the Maloca merge happened after, leaving `/maloca/*`
    // and `/v1/maloca/*` with no CORS headers and no auth enforcement on writes.
    // HumanChallenge + introspection persist in `{data dir}/humanchallenge.db` (same data
    // dir resolution as the Maloca store). On failure fall back to in-memory (None) so the
    // daemon still boots, but say so loudly.
    let hc_store = {
        let hc_dir = crate::settings::XavierSettings::resolve_state_dir();
        let _ = std::fs::create_dir_all(&hc_dir);
        let hc_path = hc_dir.join("humanchallenge.db");
        match xavier::humanchallenge::HumanChallengeStore::new(&hc_path) {
            Ok(s) => {
                xavier::humanchallenge::store::set_store_backing(
                    xavier::humanchallenge::store::StoreBacking::File,
                );
                Some(Arc::new(s))
            }
            Err(e) => {
                tracing::error!(path = %hc_path.display(), error = %e,
                    "HumanChallenge store unavailable; challenges/introspection will NOT persist");
                xavier::humanchallenge::store::set_store_backing(
                    xavier::humanchallenge::store::StoreBacking::Memory,
                );
                None
            }
        }
    };
    // Opt-in harvester: only runs when XAVIER_HC_SESSIONS_DIR points at a sessions directory
    // (no default scan of an arbitrary path on the production node).
    let hc_sessions_dir = match std::env::var("XAVIER_HC_SESSIONS_DIR") {
        Ok(dir) if !dir.trim().is_empty() => Some(dir),
        Ok(_) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            tracing::warn!("XAVIER_HC_SESSIONS_DIR is not valid UTF-8; harvester disabled");
            None
        }
        Err(std::env::VarError::NotPresent) => None,
    };
    let hc_harvester = match (hc_store.clone(), hc_sessions_dir) {
        (Some(store), Some(dir)) => {
            let config = xavier::server::maloca::HcCronBridgeConfig {
                sessions_dir: PathBuf::from(dir),
                ..Default::default()
            };
            Some(
                Arc::new(xavier::server::maloca::HcCronBridge::with_shared_store(
                    config, store,
                ))
                .start_background_worker_with_cancel(shutdown.clone()),
            )
        }
        _ => None,
    };
    let maloca_router = xavier::maloca::nested_router::<()>(maloca_store.clone())
        .merge(xavier::server::maloca::v1_maloca_router_with_maloca_store(
            hc_store,
            Some(state.workspace_dir.clone()),
            Some(maloca_store),
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            maloca_mutation_auth_middleware,
        ));

    MalocaWiring {
        router: maloca_router,
        harvester_handle: hc_harvester,
    }
}
