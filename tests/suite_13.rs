//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "test_store_path_hierarchy.rs"]
mod test_store_path_hierarchy;

#[path = "test_xtsp_benchmarks.rs"]
mod test_xtsp_benchmarks;

#[path = "tgd_consolidation_test.rs"]
mod tgd_consolidation_test;

#[path = "tgd_prune_gate_test.rs"]
mod tgd_prune_gate_test;

#[cfg(feature = "ratatui")]
#[path = "tui_screenshot_e2e.rs"]
mod tui_screenshot_e2e;

#[path = "unified_storage_validation.rs"]
mod unified_storage_validation;

#[path = "v1_api_ingest_test.rs"]
mod v1_api_ingest_test;

#[path = "v1_memories_add_path_traversal.rs"]
mod v1_memories_add_path_traversal;

#[path = "verifiable_credential_edge_test.rs"]
mod verifiable_credential_edge_test;

#[path = "verifiable_credential_test.rs"]
mod verifiable_credential_test;

#[path = "websocket_events.rs"]
mod websocket_events;

#[path = "workspace_federation_test.rs"]
mod workspace_federation_test;

#[path = "e2e/xavier_repair_verify_wave20.rs"]
mod xavier_repair_verify_wave20;

#[path = "xtsp/mod.rs"]
mod xtsp;
