//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "kernel_proxy_test.rs"]
mod kernel_proxy_test;

#[path = "keygen_e2e.rs"]
mod keygen_e2e;

#[path = "ledger_skill_entries.rs"]
mod ledger_skill_entries;

#[path = "live_system_one_integration_test.rs"]
mod live_system_one_integration_test;

#[path = "maloca_data_node_test.rs"]
mod maloca_data_node_test;

#[path = "maloca_live_sync_test.rs"]
mod maloca_live_sync_test;

#[cfg(feature = "market-v2")]
#[path = "market_v2_router_restart.rs"]
mod market_v2_router_restart;

#[path = "marketplace_api.rs"]
mod marketplace_api;

#[path = "e2e/mcp_e2e.rs"]
mod mcp_e2e;

#[path = "mcp_health_honesty_test.rs"]
mod mcp_health_honesty_test;

#[path = "e2e/memory_push_index.rs"]
mod memory_push_index;

#[path = "memory_sync_e2e.rs"]
mod memory_sync_e2e;

#[path = "memory_sync_http.rs"]
mod memory_sync_http;

#[path = "memory_sync_unified.rs"]
mod memory_sync_unified;

#[path = "repo_memory_instance_e2e.rs"]
mod repo_memory_instance_e2e;
