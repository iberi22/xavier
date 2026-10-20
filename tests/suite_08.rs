//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "nat_traversal_test.rs"]
mod nat_traversal_test;

#[path = "node_fase0_persist.rs"]
mod node_fase0_persist;

#[path = "nodes_provisioning_test.rs"]
mod nodes_provisioning_test;

#[path = "e2e/notification_e2e.rs"]
mod notification_e2e;

#[path = "notification_emitters.rs"]
mod notification_emitters;

#[path = "notifications_stream.rs"]
mod notifications_stream;

#[path = "observability_contract.rs"]
mod observability_contract;

#[path = "oss_scanner_config.rs"]
mod oss_scanner_config;

#[cfg(feature = "pageindex")]
#[path = "pageindex_eval.rs"]
mod pageindex_eval;

#[path = "panel_openui_backend.rs"]
mod panel_openui_backend;

#[path = "e2e/polygon_merkle_rollup_e2e.rs"]
mod polygon_merkle_rollup_e2e;

#[path = "production_router_smoke.rs"]
mod production_router_smoke;

#[path = "profile_vault_e2e.rs"]
mod profile_vault_e2e;

#[path = "proxy_integration.rs"]
mod proxy_integration;

#[path = "proxy_lending_integration.rs"]
mod proxy_lending_integration;
