//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "e2e/hormer_navigation_e2e.rs"]
mod hormer_navigation_e2e;

#[path = "hormer_scale.rs"]
mod hormer_scale;

#[path = "http_auth_edge_test.rs"]
mod http_auth_edge_test;

#[path = "http_pressure_wave20.rs"]
mod http_pressure_wave20;

#[path = "improve_bounded_cli_test.rs"]
mod improve_bounded_cli_test;

#[path = "ingest_cursor_d7_tests.rs"]
mod ingest_cursor_d7_tests;

#[path = "integration.rs"]
mod integration;

#[path = "issue_executor_bridge_test.rs"]
mod issue_executor_bridge_test;

#[path = "ivn_api.rs"]
mod ivn_api;

#[path = "ivn_karma.rs"]
mod ivn_karma;

#[path = "ivn_marketplace_authorization.rs"]
mod ivn_marketplace_authorization;

#[path = "ivn_storage_persistence.rs"]
mod ivn_storage_persistence;

#[path = "ivn_verdict.rs"]
mod ivn_verdict;

#[path = "kernel_mcp_test.rs"]
mod kernel_mcp_test;
