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

#[path = "codegraph_walk_hardening_e2e.rs"]
mod codegraph_walk_hardening_e2e;

#[path = "codegraph_uds_ipc_e2e.rs"]
mod codegraph_uds_ipc_e2e;

#[path = "skill_registry_unify.rs"]
mod skill_registry_unify;

#[path = "e2e/context_regen_e2e.rs"]
mod context_regen_e2e;

#[path = "context_regeneration_test.rs"]
mod context_regeneration_test;

#[path = "e2e/crypto_e2e.rs"]
mod crypto_e2e;

#[path = "backup_recover_status_e2e.rs"]
mod backup_recover_status_e2e;
#[path = "d1_clavis_vault_leak_red.rs"]
mod d1_clavis_vault_leak_red;

#[path = "incident_ticket_dedup_e2e.rs"]
mod incident_ticket_dedup_e2e;

#[path = "ticket_dedup_while_open_e2e.rs"]
mod ticket_dedup_while_open_e2e;

#[path = "space_lock_unsupported_e2e.rs"]
mod space_lock_unsupported_e2e;

#[path = "health_truth_e2e.rs"]
mod health_truth_e2e;

#[cfg(all(unix, feature = "test-utils"))]
#[path = "headless_packaging_contract_e2e.rs"]
mod headless_packaging_contract_e2e;

#[path = "mcp_telecom_tools_visibility_e2e.rs"]
mod mcp_telecom_tools_visibility_e2e;

#[path = "codegraph_indexed_symbol_project_root_e2e.rs"]
mod codegraph_indexed_symbol_project_root_e2e;
