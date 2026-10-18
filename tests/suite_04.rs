//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "espacio_isolation_e2e.rs"]
mod espacio_isolation_e2e;

#[path = "espacio_isolation_memory.rs"]
mod espacio_isolation_memory;

#[path = "espacio_links.rs"]
mod espacio_links;

#[path = "espacio_memory_encryption.rs"]
mod espacio_memory_encryption;

#[path = "espacio_production_router.rs"]
mod espacio_production_router;

#[path = "espacio_space_paths.rs"]
mod espacio_space_paths;

#[path = "fsearch_port_metrics.rs"]
mod fsearch_port_metrics;

#[path = "e2e/governance_e2e.rs"]
mod governance_e2e;

#[path = "guardian_truth_test.rs"]
mod guardian_truth_test;

#[path = "headless_api_test.rs"]
mod headless_api_test;

#[path = "headless_chat_test.rs"]
mod headless_chat_test;

#[path = "health_test.rs"]
mod health_test;

#[path = "hormer_e2e.rs"]
mod hormer_e2e;

#[path = "hormer_integration.rs"]
mod hormer_integration;
