//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "e2e/clearance_access_audit.rs"]
mod clearance_access_audit;

#[path = "clearance_audit_sqlite.rs"]
mod clearance_audit_sqlite;

#[path = "clearance_e2e.rs"]
mod clearance_e2e;

#[path = "clearance_live_e2e.rs"]
mod clearance_live_e2e;

#[path = "cli_health_stats_parity.rs"]
mod cli_health_stats_parity;

#[path = "cloud_memory_sync_cycle_test.rs"]
mod cloud_memory_sync_cycle_test;

#[path = "codegraph_dump_path_e2e.rs"]
mod codegraph_dump_path_e2e;

#[path = "codegraph_installer_e2e.rs"]
mod codegraph_installer_e2e;

#[path = "codegraph_scan_then_query.rs"]
mod codegraph_scan_then_query;

#[path = "codegraph_uds_ipc_e2e.rs"]
mod codegraph_uds_ipc_e2e;

#[path = "e2e/context_regen_e2e.rs"]
mod context_regen_e2e;

#[path = "context_regeneration_test.rs"]
mod context_regeneration_test;

#[path = "e2e/crypto_e2e.rs"]
mod crypto_e2e;

#[path = "d1_clavis_vault_leak_red.rs"]
mod d1_clavis_vault_leak_red;
