#!/bin/bash

# Remove sync routes from app.
sed -i '/\/\/ ── Memory Sync endpoints/,/\/\/ Panel UI: Vite production build/d' src/cli/server.rs

# Insert sync routes to protected_routes
sed -i '/let protected_routes = Router::new()/a\
        // ── Memory Sync endpoints ──────────────────────────────────────────\n\
        .route(\n\
            "/v1/memory/manifest",\n\
            get(crate::cli::handlers::memory::memory_manifest_handler),\n\
        )\n\
        .route(\n\
            "/v1/memory/push",\n\
            post(crate::cli::handlers::memory::memory_push_handler),\n\
        )\n\
        .route(\n\
            "/v1/memory/pull",\n\
            post(crate::cli::handlers::memory::memory_pull_handler),\n\
        )\n\
        .route(\n\
            "/v1/memory/pull-since/{workspace_id}/{since}",\n\
            get(crate::cli::handlers::memory::memory_pull_since_handler),\n\
        )\n\
        .route(\n\
            "/api/v1/memory/sync/push",\n\
            post(xavier::adapters::inbound::http::handlers::sync::sync_push_handler),\n\
        )\n\
        .route(\n\
            "/api/v1/memory/sync/pull",\n\
            post(xavier::adapters::inbound::http::handlers::sync::sync_pull_handler),\n\
        )\n\
        .route(\n\
            "/api/v1/memory/sync/status",\n\
            get(xavier::adapters::inbound::http::handlers::sync::sync_status_handler),\n\
        )\n\
        .route(\n\
            "/api/v1/memory/sync/resolve/{conflict_id}",\n\
            post(xavier::adapters::inbound::http::handlers::sync::sync_resolve_handler),\n\
        )' src/cli/server.rs
